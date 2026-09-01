//! Sağlayıcılar: sesin geldiği yer.
//!
//! Bir sağlayıcı yerel disk, Subsonic sunucusu, SoundCloud ya da (Faz 2'de)
//! alt süreç olarak çalışan bir eklenti olabilir. Çekirdek hangisi olduğunu
//! bilmez; yalnızca [`Provider`] trait'ini görür.
//!
//! ## Yetenek bayrakları neden şart
//!
//! Sağlayıcılar aynı şeyleri yapamaz. Yerel disk arama ve akış verir ama
//! uzaktan kumanda edilemez; Spotify (Faz 2, ayrı paket) yalnızca `CONTROL`
//! olacak — metadata vermez, ses akıtmaz, sadece "şunu çal" der. Trait'i tek
//! tip varsayarsak soyutlama ilk uzak oynatıcıda çöker. Bu yüzden yetenek
//! **çalışma zamanında sorulur**, derleme zamanında varsayılmaz.
//!
//! ## `uniffi` kısıtı (K7)
//!
//! Dışa açık imzalarda generic parametre, lifetime ve closure yok.
//! `Arc<dyn Provider>` callback interface olarak modellenebilir; async
//! fonksiyonlar `LookupFuture` gibi kutulanmış future döndürür ki trait
//! `dyn` uyumlu kalsın (D-006'daki `MetadataLookup` ile aynı yol).

pub mod local;
pub mod remote;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::ids::{ProviderId, ProviderTrackId};
use crate::model::TrackRef;

/// Bir sağlayıcı çağrısının dönüşü.
///
/// `async fn` yerine kutulanmış future: trait'in `dyn` uyumlu olması gerekiyor
/// (K7 / D-006). `MetadataLookup` ile aynı gerekçe.
pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Bir sağlayıcının ne yapabildiği.
///
/// Bit maskesi; `uniffi` bunu `u32` olarak taşır. Bayrak sormadan çağrılan
/// yetenek [`crate::ErrorKind::Unsupported`] döndürür — sessizce boş sonuç
/// dönmez, çünkü "yapamıyorum" ile "sonuç yok" farklı şeylerdir (K9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capabilities(u32);

impl Capabilities {
    /// Metinle parça arayabilir.
    pub const SEARCH: Self = Self(1 << 0);
    /// Katalogda gezinebilir (sanatçı → albüm → parça).
    pub const BROWSE: Self = Self(1 << 1);
    /// Çalınabilir bir ses kaynağı verebilir.
    pub const STREAM: Self = Self(1 << 2);
    /// Uzaktaki bir oynatıcıyı kumanda edebilir (Spotify Connect gibi).
    pub const CONTROL: Self = Self(1 << 3);

    /// Hiçbir yetenek.
    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// İki yeteneği birleştirir.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `wanted`'ın tamamını içeriyor mu?
    #[must_use]
    pub const fn contains(self, wanted: Self) -> bool {
        (self.0 & wanted.0) == wanted.0
    }

    /// İnsan okunur liste: `SEARCH|STREAM`.
    #[must_use]
    pub fn describe(self) -> String {
        let all = [
            (Self::SEARCH, "SEARCH"),
            (Self::BROWSE, "BROWSE"),
            (Self::STREAM, "STREAM"),
            (Self::CONTROL, "CONTROL"),
        ];
        let names: Vec<&str> = all
            .iter()
            .filter(|(flag, _)| self.contains(*flag))
            .map(|(_, name)| *name)
            .collect();
        if names.is_empty() {
            "NONE".to_owned()
        } else {
            names.join("|")
        }
    }
}

impl std::ops::BitOr for Capabilities {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::fmt::Display for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}

/// Çalınabilir bir ses kaynağı.
///
/// Faz 1'de yalnızca yerel dosya var. Uzak akış (Faz 1.3) ve eklenti
/// akışı (Faz 2) buraya varyant ekler — **K3: hiçbir varyant sunucudan
/// ses röle etmez**, hepsi istemcinin kendi çektiği kaynaktır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioSource {
    /// Yerel dosya sistemi yolu.
    LocalFile { path: std::path::PathBuf },
    /// HTTP(S) üzerinden çekilecek akış (Subsonic/Jellyfin).
    ///
    /// İstemci bunu **kendisi** çeker; `tune` sunucusu araya girmez.
    HttpStream {
        url: String,
        /// İsteğe eklenmesi gereken başlıklar (kimlik doğrulama).
        headers: Vec<HttpHeader>,
    },
}

/// Tek bir HTTP başlığı. Tanımı taşıma katmanında (`net`), burada yeniden
/// dışa açılıyor: `AudioSource` onu taşıyor ve çağıranlar iki yol
/// öğrenmek zorunda kalmasın.
pub use crate::net::HttpHeader;

/// Sağlayıcıdan dönen bir parça.
///
/// [`TrackRef`] üstverisi + sağlayıcının kendi kimliği. Kanonik kimlik
/// **burada yok**: onu kimlik zinciri üretir (K6), sağlayıcı iddia edemez.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTrack {
    pub id: ProviderTrackId,
    pub track: TrackRef,
}

/// Bir sağlayıcının kimliği ve yetenekleri.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: ProviderId,
    /// Kullanıcıya gösterilecek ad ("Yerel dosyalar").
    pub display_name: String,
    pub capabilities: Capabilities,
}

/// Bir sağlayıcının sağlık durumu. `tune provider test <ad>` bunu basar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub id: ProviderId,
    pub reachable: bool,
    /// Kaç parça göründüğü (biliniyorsa). Tanı için — K9.
    pub track_count: Option<usize>,
    /// Sorun varsa insan okunur açıklama.
    pub detail: Option<String>,
}

/// Ses kaynağı sağlayan her şey.
///
/// Yetenek bayrağı olmayan bir çağrı [`crate::ErrorKind::Unsupported`]
/// döndürmelidir — sessizce boş sonuç değil.
pub trait Provider: Send + Sync {
    /// Kimlik ve yetenekler. Senkron: çağrı yapmadan bilinmeli.
    fn info(&self) -> ProviderInfo;

    /// Sağlayıcı ayakta mı, kaç parça görüyor.
    fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth>;

    /// Metinle arama. `SEARCH` bayrağı gerekir.
    fn search<'a>(&'a self, query: &'a str, limit: usize)
    -> ProviderFuture<'a, Vec<ProviderTrack>>;

    /// Bir parçanın çalınabilir kaynağını verir. `STREAM` bayrağı gerekir.
    fn resolve_source<'a>(
        &'a self,
        id: &'a ProviderTrackId,
    ) -> ProviderFuture<'a, Option<AudioSource>>;

    /// Kataloğunu tarar ve **kalıcı depoya yazılmaya hazır** satırlar üretir.
    ///
    /// `known` daha önce görülmüş `referans → damga` eşlemesi. Sağlayıcı
    /// damgası değişmemiş öğelerin üstverisini yeniden okumaz ve
    /// [`ScannedItem::track`] alanını `None` bırakır — çağıran o satırı
    /// katalogda olduğu gibi korur. Büyük kütüphanede taramayı ucuzlatan şey
    /// budur.
    ///
    /// Varsayılan uygulama `None` döner: çoğu sağlayıcının (uzak API,
    /// kumanda) taranacak yerel bir kataloğu yoktur.
    ///
    /// Downcast yerine trait metodu: `Arc<dyn Provider>` üzerinden çağrılır
    /// ve eklentiler (Faz 2) bunu kendi yollarıyla uygulayabilir.
    fn scan_catalog<'a>(
        &'a self,
        known: &'a std::collections::HashMap<String, i64>,
    ) -> ProviderFuture<'a, Option<CatalogScan>> {
        let _ = known;
        Box::pin(std::future::ready(Ok(None)))
    }

    /// Katalog `since_ms`'ten beri değişmiş olabilir mi? (D-025)
    ///
    /// Tam taramadan **çok daha ucuz** olmalı; amacı "taramaya değer mi"
    /// sorusunu cevaplamak. Üç ayrı cevap var ve üçü de farklı şeydir (K9):
    ///
    /// - `Some(true)` — değişmiş, taramaya değer.
    /// - `Some(false)` — değişmemiş, tarama atlanabilir.
    /// - `None` — **bilmiyorum.** Varsayılan bu; uzak sağlayıcı ucuz bir
    ///   değişiklik damgası sunmuyor ve "değişmedi" demek yanlış olurdu.
    ///
    /// Yine downcast yerine trait metodu: eklentiler (Faz 2) kendi ucuz
    /// damgalarını verebilsin.
    fn catalog_changed_since(&self, since_ms: i64) -> ProviderFuture<'_, Option<bool>> {
        let _ = since_ms;
        Box::pin(std::future::ready(Ok(None)))
    }
}

/// Bir taramanın sonucu: satırlar + ne olduğunun özeti.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogScan {
    pub tracks: Vec<ScannedItem>,
    pub summary: ScanSummary,
}

/// Taramada görülen tek bir öğe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedItem {
    /// Sağlayıcının bu öğe için kimliği.
    pub id: ProviderTrackId,
    /// Değişiklik damgası (yerel dosyada mtime). Yoksa her tarama yeniden okur.
    pub mtime_ms: Option<i64>,
    /// Okunan üstveri. `None` ise öğe değişmemiş — katalogdaki hâli korunur.
    pub track: Option<TrackRef>,
    /// Üstveri etiketlerden mi geldi (dosya adından/tahminden değil).
    pub from_tags: bool,
}

pub use local::ScanSummary;

/// Kayıtlı sağlayıcılar. `tune provider list` bunu okur.
///
/// Faz 2'de eklentiler buraya alt süreç olarak katılacak (K5); kayıt defteri
/// arayüzü o gün değişmesin diye bugün de aynı yüzeyi kullanıyoruz.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("count", &self.providers.len())
            .finish()
    }
}

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bir sağlayıcı ekler.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
    }

    /// Kayıtlı sağlayıcıların bilgileri.
    #[must_use]
    pub fn list(&self) -> Vec<ProviderInfo> {
        self.providers.iter().map(|p| p.info()).collect()
    }

    /// Kayıtlı bütün sağlayıcılar.
    #[must_use]
    pub fn all(&self) -> Vec<Arc<dyn Provider>> {
        self.providers.iter().map(Arc::clone).collect()
    }

    /// Ada göre bulur.
    #[must_use]
    pub fn get(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        self.providers
            .iter()
            .find(|p| &p.info().id == id)
            .map(Arc::clone)
    }

    /// Belirli bir yeteneğe sahip sağlayıcılar.
    #[must_use]
    pub fn with_capability(&self, wanted: Capabilities) -> Vec<Arc<dyn Provider>> {
        self.providers
            .iter()
            .filter(|p| p.info().capabilities.contains(wanted))
            .map(Arc::clone)
            .collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }
}

/// Yapılandırmadan varsayılan sağlayıcıları kurar.
///
/// Yerel dosya sağlayıcı (D-017) + `servers.json`'daki uzak sunucular
/// (D-019), bu derlemenin varsayılan HTTP istemcisiyle (D-020). Faz 2'de
/// eklentiler buraya katılacak; çağıranların (CLI, GUI, mobil) imzası
/// değişmesin diye kurulum bugünden çekirdekte.
///
/// # Errors
/// Sunucu kayıt dosyası okunamaz ya da bozuksa.
pub fn default_registry(config: &crate::config::Config) -> crate::Result<ProviderRegistry> {
    let servers = remote::load_servers(&config.servers_path())?;
    let http = if servers.is_empty() {
        // Kayıtlı sunucu yoksa istemci kurmaya gerek yok: `http-client`
        // kapalı bir derlemede de `provider list` çalışmalı.
        None
    } else {
        match crate::net::default_http_client() {
            Ok(client) => Some(client),
            Err(err) => {
                // Sessizce atlamıyoruz: kullanıcının kayıtlı sunucusu var
                // ama bu derleme ağa çıkamıyor (K9).
                tracing::warn!(
                    error = %err.chain_text().replace('\n', " "),
                    servers = servers.len(),
                    "kayıtlı uzak sunucular atlandı"
                );
                None
            }
        }
    };
    registry_with_http(config, http)
}

/// Sağlayıcıları verilen HTTP taşımasıyla kurar.
///
/// GUI ve mobil bunu çağırır: kendi HTTP yığınlarını `Arc<dyn HttpClient>`
/// olarak verip TLS ağacını ikinci kez taşımazlar (D-020). `http` `None` ise
/// yalnızca yerel sağlayıcı kurulur.
///
/// # Errors
/// Sunucu kayıt dosyası okunamaz ya da bozuksa.
pub fn registry_with_http(
    config: &crate::config::Config,
    http: Option<Arc<dyn crate::net::HttpClient>>,
) -> crate::Result<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(local::LocalProvider::new(config.music_dirs())));

    if let Some(http) = http {
        for server in remote::load_servers(&config.servers_path())? {
            registry.register(remote::provider_for(&server, Arc::clone(&http)));
        }
    }

    // Onaylı eklentiler (Faz 2, §2.1). Hiçbiri burada **başlatılmıyor**:
    // her eklenti ilk çağrısında kendi sürecini açar, `provider list` süreç
    // açmadan çalışır.
    let (plugins, summary) = crate::plugin::load(config)?;
    for plugin in plugins {
        registry.register(plugin);
    }
    if summary.discovered > 0 {
        // Yüklenmeyenler sessiz kalmasın: sebepleri `tune plugin list`'te.
        tracing::debug!(
            discovered = summary.discovered,
            ready = summary.ready,
            awaiting_approval = summary.awaiting_approval,
            disabled = summary.disabled,
            incompatible = summary.incompatible,
            broken = summary.broken,
            "eklentiler tarandı"
        );
    }
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_compose_and_describe() {
        let caps = Capabilities::SEARCH | Capabilities::STREAM;
        assert!(caps.contains(Capabilities::SEARCH));
        assert!(caps.contains(Capabilities::STREAM));
        assert!(!caps.contains(Capabilities::CONTROL));
        assert_eq!(caps.describe(), "SEARCH|STREAM");
        assert_eq!(Capabilities::NONE.describe(), "NONE");
    }

    #[test]
    fn contains_requires_all_requested_flags() {
        let caps = Capabilities::SEARCH;
        let wanted = Capabilities::SEARCH | Capabilities::STREAM;
        assert!(
            !caps.contains(wanted),
            "eksik bayrak varken contains false olmalı"
        );
    }

    #[test]
    fn capabilities_survive_a_json_round_trip() {
        // `uniffi` bunu u32 olarak taşıyacak; serde gösterimi de sayı olmalı.
        let caps = Capabilities::SEARCH | Capabilities::CONTROL;
        let json = serde_json::to_string(&caps).unwrap();
        assert_eq!(json, "9", "1 | 8 = 9");
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
    }

    #[test]
    fn audio_source_is_tagged_in_json() {
        let source = AudioSource::LocalFile {
            path: std::path::PathBuf::from("/muzik/a.flac"),
        };
        let json = serde_json::to_string(&source).unwrap();
        assert!(json.contains("\"kind\":\"local_file\""), "{json}");
        let back: AudioSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }
}
