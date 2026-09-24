//! Torrent motoru: `librqbit` oturumu, katalog ve dosya listesi.
//!
//! ## Neden bir katalog dosyası var
//!
//! `search` bir magnet döndürür, `resolve_source` **başka bir süreç
//! koşumunda** gelebilir (çekirdek eklentiyi kapatıp yeniden açar). Elimizde
//! yalnızca infohash kalırsa tracker listesini kaybederiz ve torrent'i
//! bulmak DHT'ye kalır — bazen dakikalar, bazen hiç. Bu yüzden aramada
//! görülen her yayımın magnet'i `catalog.json`'a yazılıyor.
//!
//! ## Neden her çağrının bir bütçesi var
//!
//! Çekirdeğin çağrı zaman aşımı 20 sn (`client::CALL_TIMEOUT`). Soğuk bir
//! magnet'in üstverisini çözmek bundan uzun sürebilir. Sabit bir `sleep`
//! yerine **gerçek hazır olma kontrolü** yapıyoruz (bash prototipinin dersi,
//! PLAN §2.4) ve bütçe dolduğunda "henüz hazır değil, arka planda devam
//! ediyor, tekrar deneyin" diyoruz — sessizce boş sonuç değil (K9).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::release;
use crate::rpc::{PluginError, Result, chain_text, err};

/// `librqbit`'in kendi `ManagedTorrentHandle` takma adı dışa açık değil;
/// aynı tipi burada tutuyoruz.
pub type TorrentHandle = Arc<ManagedTorrent>;

/// Üstverinin çözülmesi için tanınan süre. Çekirdeğin 20 sn'lik çağrı zaman
/// aşımının altında kalmalı ki zaman aşımı **bizim** tarafımızda, açıklamalı
/// bir cevap olarak görünsün.
pub const METADATA_BUDGET: std::time::Duration = std::time::Duration::from_secs(14);

/// Bir torrent'in içindeki tek ses dosyası.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFile {
    /// Torrent içindeki dosya sırası. Kimliğin ikinci parçası bu.
    pub index: usize,
    pub relative_path: String,
    pub file_name: String,
    pub len: u64,
}

/// Katalogda tutulan tek kayıt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub title: String,
    /// Magnet ya da `.torrent` adresi — tracker listesini taşıyan hâli.
    pub source_url: String,
    #[serde(default)]
    pub indexer: Option<String>,
}

/// `catalog.json`. Küçük, insan okunur, ve kaybolursa yalnızca hız kaybı.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Catalog {
    #[serde(default)]
    entries: BTreeMap<String, CatalogEntry>,
}

pub struct Engine {
    session: Arc<Session>,
    download_dir: PathBuf,
    catalog_path: PathBuf,
    catalog: Mutex<Catalog>,
}

impl Engine {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        let download_dir = data_dir.join("downloads");
        std::fs::create_dir_all(&download_dir).map_err(|error| {
            PluginError::new(format!(
                "indirme dizini oluşturulamadı ({}): {error}",
                download_dir.display()
            ))
        })?;

        let session = Session::new(download_dir.clone()).await.map_err(|error| {
            PluginError::new(format!("torrent oturumu açılamadı: {}", chain_text(&error)))
        })?;

        let catalog_path = data_dir.join("catalog.json");
        let catalog = read_catalog(&catalog_path);
        Ok(Self {
            session,
            download_dir,
            catalog_path,
            catalog: Mutex::new(catalog),
        })
    }

    pub fn download_dir(&self) -> &std::path::Path {
        &self.download_dir
    }

    /// Aramada görülen yayımları kataloğa yazar. Yazılamazsa iş durmaz ama
    /// **sessiz kalmaz** — sonraki `resolve_source` yavaşlayacak demektir.
    pub async fn remember(&self, entries: Vec<(String, CatalogEntry)>) {
        if entries.is_empty() {
            return;
        }
        let mut catalog = self.catalog.lock().await;
        for (infohash, entry) in entries {
            catalog.entries.insert(infohash, entry);
        }
        if let Err(error) = write_catalog(&self.catalog_path, &catalog) {
            crate::rpc::log(
                "warn",
                format!("katalog yazılamadı, sonraki çalma yavaşlayabilir: {error}"),
            );
        }
    }

    pub async fn lookup(&self, infohash: &str) -> Option<CatalogEntry> {
        self.catalog.lock().await.entries.get(infohash).cloned()
    }

    /// Infohash için bir kaynak adresi üretir.
    ///
    /// Katalogda varsa tracker'lı magnet, yoksa çıplak magnet — çıplak hâl
    /// yalnızca DHT'ye dayanır ve bunu söylüyoruz, çünkü "hiç peer bulunamadı"
    /// tanısının sebebi çoğu zaman budur.
    pub async fn source_for(&self, infohash: &str) -> (String, bool) {
        match self.lookup(infohash).await {
            Some(entry) => (entry.source_url, true),
            None => (format!("magnet:?xt=urn:btih:{infohash}"), false),
        }
    }

    /// Torrent'i oturuma ekler (ya da zaten ekliyse tutamacını verir) ve
    /// üstverisi çözülene kadar bütçe kadar bekler.
    pub async fn handle(&self, infohash: &str, source_url: &str) -> Result<TorrentHandle> {
        let options = AddTorrentOptions {
            // Her torrent kendi dizinine (PLAN §2.4, bash prototipinin dersi):
            // iki yayımın aynı dosya adını taşıması sık, ve üst üste yazmak
            // sessiz veri kaybıdır.
            output_folder: Some(self.download_dir.join(infohash).display().to_string()),
            overwrite: true,
            ..Default::default()
        };
        let add = add_torrent_from(source_url)?;
        // **Bütçe `add_torrent`'ı da kapsamak zorunda.** Bir magnet'te
        // üstveriyi çözen `wait_until_initialized` değil, `add_torrent`'ın
        // kendisi: peer bulunamazsa orada süresizce bekler. Yalnızca
        // beklemeyi saymak, soğuk bir magnet'te eklentiyi çekirdeğin 20 sn'lik
        // zaman aşımına düşürüyordu — yani kullanıcı "eklenti takıldı" görüp
        // sebebini hiç öğrenemiyordu (K9).
        let acquire = async {
            let response = self
                .session
                .add_torrent(add, Some(options))
                .await
                .map_err(|error| {
                    PluginError::new(format!(
                        "torrent eklenemedi ({infohash}): {}",
                        chain_text(&error)
                    ))
                })?;
            let Some(handle) = response.into_handle() else {
                return err(format!(
                    "torrent yalnızca listelendi, çalışır hâle gelmedi ({infohash})"
                ));
            };
            handle.wait_until_initialized().await.map_err(|error| {
                PluginError::new(format!(
                    "torrent üstverisi çözülemedi ({infohash}): {}",
                    chain_text(&error)
                ))
            })?;
            Ok(handle)
        };

        match tokio::time::timeout(METADATA_BUDGET, acquire).await {
            Ok(result) => result,
            Err(_) => err(format!(
                "torrent üstverisi {} sn'de gelmedi ({infohash}); peer bulunamamış \
                 olabilir. Kaynak tracker taşımıyorsa yalnızca DHT'ye kalıyoruz — \
                 magnet'i aratmak (`headshell provider search torrent <magnet>`) tracker \
                 listesini kataloğa yazar. İndirme arka planda sürüyor, tekrar deneyin.",
                METADATA_BUDGET.as_secs()
            )),
        }
    }

    /// Torrent'in içindeki ses dosyaları, torrent'teki sıralarıyla.
    pub fn audio_files(handle: &TorrentHandle) -> Result<Vec<AudioFile>> {
        let Some(metadata) = handle.metadata.load_full() else {
            return err("torrent üstverisi henüz yok");
        };
        let mut files = Vec::new();
        for (index, info) in metadata.file_infos.iter().enumerate() {
            if !release::is_audio(&info.relative_filename) {
                continue;
            }
            let file_name = info
                .relative_filename
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            files.push(AudioFile {
                index,
                relative_path: info.relative_filename.display().to_string(),
                file_name,
                len: info.len,
            });
        }
        Ok(files)
    }

    /// İnsan okunur ilerleme özeti — peer sayısı ve hız dahil (PLAN §2.4).
    pub fn progress(handle: &TorrentHandle) -> String {
        let stats = handle.stats();
        let mut text = format!("{} / {} bayt", stats.progress_bytes, stats.total_bytes);
        if let Some(live) = stats.live.as_ref() {
            text.push_str(&format!(
                ", {} peer, {}",
                live.snapshot.peer_stats.live, live.download_speed
            ));
        }
        if let Some(error) = stats.error.as_ref() {
            text.push_str(&format!(", hata: {error}"));
        }
        text
    }
}

/// Kaynağı `librqbit`'in anladığı biçime çevirir.
///
/// `magnet:` / `http(s):` doğrudan gider; onun dışındaki her şey **yerel bir
/// `.torrent` dosyası** olarak okunur. Böylece elinde dosya olan bir kullanıcı
/// da (ve testler de) aynı yoldan geçer. Tanınmayan bir şeyi adres sanıp
/// `librqbit`'in "geçersiz URL" hatasına bırakmak, tanıyı yanlış yere koyardı.
fn add_torrent_from(source: &str) -> Result<AddTorrent<'_>> {
    if librqbit::SUPPORTED_SCHEMES
        .iter()
        .any(|scheme| source.starts_with(scheme))
    {
        return Ok(AddTorrent::from_url(source));
    }
    let path = std::path::Path::new(source);
    if !path.is_file() {
        return err(format!(
            "kaynak ne magnet/http adresi ne de var olan bir .torrent dosyası: {source}"
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| PluginError::new(format!("`.torrent` okunamadı ({source}): {error}")))?;
    Ok(AddTorrent::from_bytes(bytes))
}

fn read_catalog(path: &std::path::Path) -> Catalog {
    let Ok(raw) = std::fs::read_to_string(path) else {
        // Dosya yoksa boş katalog doğru cevap; okunamıyorsa da devam ederiz
        // ama bunu ilk yazmada fark edeceğiz.
        return Catalog::default();
    };
    match serde_json::from_str(&raw) {
        Ok(catalog) => catalog,
        Err(error) => {
            crate::rpc::log(
                "warn",
                format!("katalog bozuk, boş sayılıyor ({}): {error}", path.display()),
            );
            Catalog::default()
        }
    }
}

fn write_catalog(path: &std::path::Path, catalog: &Catalog) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(catalog).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, raw)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "headshell-torrent-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_catalog_round_trips_through_the_file() {
        let dir = temp_dir("katalog");
        let path = dir.join("catalog.json");
        let mut catalog = Catalog::default();
        catalog.entries.insert(
            "a".repeat(40),
            CatalogEntry {
                title: "Bir Yayım".to_owned(),
                source_url: "magnet:?xt=urn:btih:x&tr=udp://t".to_owned(),
                indexer: Some("ornek".to_owned()),
            },
        );
        write_catalog(&path, &catalog).unwrap();

        let read = read_catalog(&path);
        assert_eq!(read.entries.len(), 1);
        assert_eq!(
            read.entries[&"a".repeat(40)].source_url,
            "magnet:?xt=urn:btih:x&tr=udp://t"
        );
    }

    #[test]
    fn a_missing_catalog_is_empty_not_an_error() {
        let dir = temp_dir("yok");
        assert!(read_catalog(&dir.join("catalog.json")).entries.is_empty());
    }

    #[test]
    fn a_corrupt_catalog_does_not_take_the_plugin_down() {
        let dir = temp_dir("bozuk");
        let path = dir.join("catalog.json");
        std::fs::write(&path, "{ bu json değil").unwrap();
        assert!(read_catalog(&path).entries.is_empty());
    }

    #[test]
    fn the_metadata_budget_stays_under_the_cores_call_timeout() {
        // Bütçe çekirdeğin zaman aşımını geçerse hata **bizim** tarafımızda
        // açıklamalı bir cevap olarak değil, çekirdekte "eklenti takıldı"
        // olarak görünür — ve kullanıcı sebebi öğrenemez.
        assert!(
            METADATA_BUDGET < headshell_core::plugin::client::CALL_TIMEOUT,
            "bütçe {METADATA_BUDGET:?}, zaman aşımı {:?}",
            headshell_core::plugin::client::CALL_TIMEOUT
        );
    }
}
