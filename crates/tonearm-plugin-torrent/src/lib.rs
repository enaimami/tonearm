//! Torrent sağlayıcı eklentisi (protokol api 1) — Faz 2 §2.4, D-047.
//!
//! Çekirdeğin içinde değil, **alt süreç** olarak çalışır (K5). Sebep ölçüldü:
//! `librqbit` `tonearm-core`'un bağımlılık ağacına 179 crate ekliyordu (77 → 256)
//! ve o ağaç `uniffi` ile mobile de gidecekti. Buradan gitmiyor.
//!
//! ## İki adımlı arama
//!
//! Torznab bir **yayım** (release) döndürür, bir parça değil — genelde bir
//! albüm. `WireTrack` ise bir parça. api 1'i büyütmeden çözümü iki adım:
//!
//! 1. `search "<sorgu>"` → yayımlar; her birinin kimliği `<infohash>`.
//! 2. `search "<infohash>"` → o torrent'in ses dosyaları; kimlikler
//!    `<infohash>/<dosya sırası>`.
//!
//! `resolve_source` ikisini de kabul eder. Tek ses dosyası olan bir yayımda
//! çıplak infohash doğrudan çalar; birden çok dosya varsa **tahmin etmez**,
//! ne yapılacağını söyleyen bir hata döner (K9).
//!
//! ## Ses nasıl teslim edilir
//!
//! `resolve_source`, `127.0.0.1`'de dinleyen kendi HTTP sunucumuzun adresini
//! `HttpStream` olarak döndürür (bkz. [`stream`]). İndirmenin bitmesini
//! beklemez: `librqbit` parça önceliğini okuma konumuna göre ayarlıyor.
//!
//! ## TODO: AFTER FIRST RELEASE — dağıtım D-049'u ihlal ediyor
//!
//! Bu eklenti kullanıcıya `cargo build --release -p tonearm-plugin-torrent`
//! yaptırıyor, yani **bir Rust araç zinciri kurduruyor.** D-049 hiçbir
//! eklentinin sistem çapında kurulum istememesini şart koşuyor ve depodaki
//! dört eklentiden D-055'ten sonra bunu ihlal eden **tek** şey burası:
//! ötekiler betik, motorun Python'undan geçiyorlar; bu bir ikili, geçemiyor.
//!
//! Bir zamanlar çözüm "çekirdeğe feature'lı sağlayıcı olarak taşı" idi
//! (D-050 S3). **D-056 o kararı iptal etti**: taşınacak şey 2.335 satır
//! kaynak + 647 satır test, çalışan bir eklenti — ilk sürümden önce sökmenin
//! karşılığı yok.
//!
//! Açık kalan soru **dağıtım**, mimari değil: platform başına önceden
//! derlenmiş yayın çıktısı mı, yoksa "kaynaktan derle" mi kalacak. İlk
//! sürümden sonra karara bağlanacak — PLAN §2.8 madde 5.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod engine;
pub mod release;
pub mod rpc;
pub mod stream;
pub mod torznab;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::OnceCell;

use tonearm_core::plugin::protocol::{
    HandshakeParams, HealthResult, PLUGIN_API, ResolveSourceParams, SearchParams, SearchResult,
    WireTrack, method,
};
use tonearm_core::provider::AudioSource;

use crate::engine::{CatalogEntry, Engine};
use crate::rpc::{Result, err};
use crate::stream::StreamServer;

pub const PLUGIN_NAME: &str = "torrent";
const DISPLAY_NAME: &str = "Torrent";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sır ad alanındaki anahtarlar (D-042). Değerleri hiçbir yerde basılmıyor.
const SECRET_TORZNAB_URL: &str = "torznab_url";
const SECRET_TORZNAB_KEY: &str = "torznab_api_key";

/// Torznab yapılandırılmamışken `search`in verdiği cevap.
///
/// Boş küme **değil**: "bakmadım" ile "bulamadım" ayrı tanılardır (K9).
const TORZNAB_MISSING: &str = concat!(
    "Torznab yapılandırılmamış — arama yapılamaz (indirme ve çalma çalışır). ",
    "Prowlarr ya da Jackett kurup şunları verin: ",
    "`tonearm secret set plugin:torrent torznab_url` ",
    "(ör. http://127.0.0.1:9696/1/api) ve ",
    "`tonearm secret set plugin:torrent torznab_api_key`."
);

pub struct App {
    secrets: BTreeMap<String, String>,
    data_dir: PathBuf,
    engine: OnceCell<Arc<Engine>>,
    server: OnceCell<Arc<StreamServer>>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        Self {
            secrets: BTreeMap::new(),
            data_dir: PathBuf::from("."),
            engine: OnceCell::new(),
            server: OnceCell::new(),
        }
    }

    pub fn torznab(&self) -> Result<torznab::Torznab> {
        let Some(url) = self.secrets.get(SECRET_TORZNAB_URL).map(String::as_str) else {
            return err(TORZNAB_MISSING);
        };
        torznab::Torznab::new(
            url,
            self.secrets
                .get(SECRET_TORZNAB_KEY)
                .map(String::as_str)
                .unwrap_or_default(),
        )
    }

    /// Motor ilk ihtiyaç duyulduğunda kuruluyor — el sıkışmada değil.
    ///
    /// El sıkışmanın zaman aşımı 5 sn ve bir torrent oturumu açmak (port
    /// bağlama, DHT ön yükleme) bundan uzun sürebilir. Orada kurmak,
    /// eklentiyi ağ yavaşken **yüklenemez** hâle getirirdi.
    pub async fn engine(&self) -> Result<Arc<Engine>> {
        self.engine
            .get_or_try_init(|| async { Engine::new(self.data_dir.clone()).await.map(Arc::new) })
            .await
            .cloned()
    }

    pub async fn server(&self) -> Result<Arc<StreamServer>> {
        let engine = self.engine().await?;
        self.server
            .get_or_try_init(|| async { StreamServer::spawn(engine).await })
            .await
            .cloned()
    }
}

pub async fn dispatch(
    app: &mut App,
    method_name: &str,
    params: serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    let value = match method_name {
        method::HANDSHAKE => handshake(app, params)?,
        method::HEALTH => health(app).await?,
        method::SEARCH => search(app, params).await?,
        method::RESOLVE_SOURCE => resolve_source(app, params).await?,
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub fn handshake(app: &mut App, params: serde_json::Value) -> Result<serde_json::Value> {
    let params: HandshakeParams = serde_json::from_value(params)
        .map_err(|error| rpc::PluginError::new(format!("el sıkışma gövdesi okunamadı: {error}")))?;
    if params.api != PLUGIN_API {
        return err(format!(
            "protokol sürümü uyuşmuyor: çekirdek {}, eklenti {PLUGIN_API}",
            params.api
        ));
    }
    app.secrets = params.secrets;
    app.data_dir = PathBuf::from(params.data_dir);

    to_value(&serde_json::json!({
        "api": PLUGIN_API,
        "name": PLUGIN_NAME,
        "display_name": DISPLAY_NAME,
        "plugin_version": VERSION,
        "capabilities": ["search", "stream"],
    }))
}

/// Sağlık: **iki ayrı** şey ölçülüyor ve ayrı ayrı raporlanıyor.
///
/// Torznab'a ulaşamamak aramanın çalışmadığı anlamına gelir; torrent motoru
/// yine ayakta olabilir ve elde infohash olan bir parça yine çalar. İkisini
/// tek bir `reachable` bayrağına indirmek, kullanıcıya yanlış şeyi tamir
/// ettirir (K9).
pub async fn health(app: &App) -> Result<serde_json::Value> {
    let mut notes = Vec::new();
    let mut reachable = true;

    match app.torznab() {
        Ok(client) => match client.caps().await {
            Ok(detail) => notes.push(format!("arama: {detail}")),
            Err(error) => {
                reachable = false;
                notes.push(format!("arama çalışmıyor: {error}"));
            }
        },
        Err(error) => {
            reachable = false;
            notes.push(format!("arama yapılandırılmamış: {error}"));
        }
    }

    match app.engine().await {
        Ok(engine) => notes.push(format!(
            "torrent motoru hazır, indirme dizini {}",
            engine.download_dir().display()
        )),
        Err(error) => {
            reachable = false;
            notes.push(format!("torrent motoru açılamadı: {error}"));
        }
    }

    let result = HealthResult {
        reachable,
        // Torrent'in bir kataloğu yok: bir sayı vermek uydurmak olurdu.
        track_count: None,
        detail: Some(notes.join(" | ")),
    };
    to_value(&result)
}

pub async fn search(app: &App, params: serde_json::Value) -> Result<serde_json::Value> {
    let params: SearchParams = serde_json::from_value(params)
        .map_err(|error| rpc::PluginError::new(format!("arama gövdesi okunamadı: {error}")))?;
    let query = params.query.trim();
    if query.is_empty() {
        return to_value(&SearchResult { tracks: Vec::new() });
    }

    // İkinci adım: sorgu bir infohash ise, o torrent'in içindeki dosyalar.
    let candidate = query.to_ascii_lowercase();
    if torznab::is_infohash(&candidate) {
        return search_inside(app, &candidate).await;
    }
    if let Some(hash) = torznab::infohash_from_magnet(query) {
        // Magnet yapıştıran kullanıcı: kataloğa yazıp içine bakıyoruz.
        let engine = app.engine().await?;
        engine
            .remember(vec![(
                hash.clone(),
                CatalogEntry {
                    title: hash.clone(),
                    source_url: query.to_owned(),
                    indexer: None,
                },
            )])
            .await;
        return search_inside(app, &hash).await;
    }

    search_releases(app, query, params.limit).await
}

/// Birinci adım: Torznab'da yayım ara.
pub async fn search_releases(app: &App, query: &str, limit: usize) -> Result<serde_json::Value> {
    let client = app.torznab()?;
    let outcome = client.search(query, limit).await?;

    if outcome.dropped_unidentifiable > 0 {
        rpc::log(
            "warn",
            format!(
                "{} sonuç infohash taşımadığı için düşürüldü (çalınamazlardı)",
                outcome.dropped_unidentifiable
            ),
        );
    }

    let engine = app.engine().await?;
    let mut remember = Vec::new();
    let mut tracks = Vec::new();
    for found in &outcome.releases {
        let Some(source_url) = found.source_url() else {
            continue;
        };
        remember.push((
            found.infohash.clone(),
            CatalogEntry {
                title: found.title.clone(),
                source_url: source_url.to_owned(),
                indexer: found.indexer.clone(),
            },
        ));
        let parsed = release::parse_release_name(&found.title);
        tracks.push(WireTrack {
            id: found.infohash.clone(),
            artist: parsed.artist,
            // Yayım adı albüm; başlık alanına da onu koyuyoruz çünkü bu
            // satır bir parça değil, bir yayım. İçine `search "<infohash>"`
            // ile inilir.
            title: parsed.title.clone(),
            album: Some(parsed.title),
            // Bir yayımın süresi yok. Uydurmak, kimlik zincirinin süre
            // eşitlik-bozucusunu (D-046 ek 2) yanlış yönlendirirdi.
            duration_ms: None,
            isrc: None,
        });
    }
    engine.remember(remember).await;

    to_value(&SearchResult { tracks })
}

/// İkinci adım: bir torrent'in içindeki ses dosyaları.
pub async fn search_inside(app: &App, infohash: &str) -> Result<serde_json::Value> {
    let engine = app.engine().await?;
    let (source_url, from_catalog) = engine.source_for(infohash).await;
    if !from_catalog {
        rpc::log(
            "info",
            format!("{infohash} katalogda yok; tracker listesi olmadan yalnızca DHT ile aranacak"),
        );
    }
    let handle = engine.handle(infohash, &source_url).await?;
    let files = Engine::audio_files(&handle)?;
    if files.is_empty() {
        return err(format!(
            "torrent'te ses dosyası yok ({infohash}); durum: {}",
            Engine::progress(&handle)
        ));
    }

    let release_name = engine
        .lookup(infohash)
        .await
        .map(|entry| entry.title)
        .unwrap_or_default();
    let parsed = release::parse_release_name(&release_name);

    let tracks = files
        .iter()
        .map(|file| {
            let (title, _track_no) = release::parse_file_name(&file.file_name);
            WireTrack {
                id: format!("{infohash}/{}", file.index),
                artist: parsed.artist.clone(),
                title,
                album: (!parsed.title.is_empty()).then(|| parsed.title.clone()),
                // Süre torrent üstverisinde yok — dosyayı çözmeden bilinemez.
                // Kimlik zinciri onu dosyadan (parmak izi yolu) öğrenir.
                duration_ms: None,
                isrc: None,
            }
        })
        .collect();

    to_value(&SearchResult { tracks })
}

pub async fn resolve_source(app: &App, params: serde_json::Value) -> Result<serde_json::Value> {
    let params: ResolveSourceParams = serde_json::from_value(params)
        .map_err(|error| rpc::PluginError::new(format!("çözümleme gövdesi okunamadı: {error}")))?;
    let raw = params.id.trim();
    if raw.is_empty() {
        return err("parça kimliği boş");
    }

    let (infohash, wanted_index) = match raw.split_once('/') {
        Some((hash, index)) => {
            let parsed: usize = index
                .parse()
                .map_err(|_| rpc::PluginError::new(format!("dosya sırası sayı değil: {index}")))?;
            (hash.to_ascii_lowercase(), Some(parsed))
        }
        None => (raw.to_ascii_lowercase(), None),
    };
    if !torznab::is_infohash(&infohash) {
        return err(format!("kimlik bir infohash değil: {infohash}"));
    }

    let engine = app.engine().await?;
    let (source_url, _) = engine.source_for(&infohash).await;
    let handle = engine.handle(&infohash, &source_url).await?;
    let files = Engine::audio_files(&handle)?;

    let index = match wanted_index {
        Some(index) => {
            if !files.iter().any(|file| file.index == index) {
                // "Yok" bir cevaptır, hata değil (protokol §resolve_source).
                return to_value(&tonearm_core::plugin::protocol::ResolveSourceResult {
                    source: None,
                });
            }
            index
        }
        None => match files.as_slice() {
            [] => {
                return err(format!(
                    "torrent'te ses dosyası yok ({infohash}); durum: {}",
                    Engine::progress(&handle)
                ));
            }
            [single] => single.index,
            many => {
                // Hangisi olduğunu bilmiyoruz ve **tahmin etmiyoruz** (K9):
                // ilk dosyayı seçmek, kullanıcıya sessizce yanlış parçayı çalar.
                let listing = many
                    .iter()
                    .take(10)
                    .map(|file| format!("{}: {}", file.index, file.file_name))
                    .collect::<Vec<_>>()
                    .join(", ");
                let extra = if many.len() > 10 {
                    format!(" (+{} dosya daha)", many.len() - 10)
                } else {
                    String::new()
                };
                return err(format!(
                    "bu yayımda {} ses dosyası var, hangisi olduğunu söylemediniz. \
                     `tonearm provider search torrent {infohash}` dosyaları listeler; \
                     kimlik `{infohash}/<sıra>` olur. Dosyalar — {listing}{extra}",
                    many.len()
                ));
            }
        },
    };

    let server = app.server().await?;
    let url = server.url_for(&infohash, index);
    rpc::log(
        "info",
        format!(
            "{infohash}/{index} akışa hazırlanıyor: {}",
            Engine::progress(&handle)
        ),
    );

    to_value(&tonearm_core::plugin::protocol::ResolveSourceResult {
        source: Some(AudioSource::HttpStream {
            url,
            headers: Vec::new(),
        }),
    })
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value)
        .map_err(|error| rpc::PluginError::new(format!("cevap serileştirilemedi: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missing_torznab_message_tells_the_user_exactly_what_to_type() {
        assert!(TORZNAB_MISSING.contains("tonearm secret set plugin:torrent torznab_url"));
        assert!(TORZNAB_MISSING.contains("torznab_api_key"));
        // "Arama yok" ile "hiçbir şey çalışmıyor" karıştırılmamalı.
        assert!(TORZNAB_MISSING.contains("çalma çalışır"));
    }

    #[test]
    fn an_unconfigured_search_is_an_error_not_an_empty_result() {
        let app = App::new();
        let error = app.torznab().unwrap_err();
        assert!(error.to_string().contains("yapılandırılmamış"), "{error}");
    }

    #[test]
    fn the_handshake_answers_with_the_name_the_manifest_declares() {
        let mut app = App::new();
        let value = handshake(
            &mut app,
            serde_json::json!({
                "api": PLUGIN_API,
                "host": {"name": "tonearm", "version": "0.0.1"},
                "data_dir": "/tmp/x",
                "secrets": {"torznab_url": "https://x/api"},
                "permissions": {"net": [], "fs": []},
            }),
        )
        .unwrap();
        assert_eq!(value["name"], PLUGIN_NAME);
        assert_eq!(value["api"], PLUGIN_API);
        assert_eq!(
            value["capabilities"],
            serde_json::json!(["search", "stream"])
        );
        assert_eq!(app.data_dir, PathBuf::from("/tmp/x"));
        assert!(app.torznab().is_ok(), "sır el sıkışmadan alınmalı");
    }

    #[test]
    fn a_version_mismatch_is_refused_with_both_numbers_visible() {
        let mut app = App::new();
        let error = handshake(
            &mut app,
            serde_json::json!({
                "api": PLUGIN_API + 7,
                "host": {"name": "tonearm", "version": "0.0.1"},
                "data_dir": "/tmp/x",
                "secrets": {},
                "permissions": {"net": [], "fs": []},
            }),
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains(&(PLUGIN_API + 7).to_string()), "{text}");
        assert!(text.contains(&PLUGIN_API.to_string()), "{text}");
    }

    #[tokio::test]
    async fn an_empty_query_is_an_empty_result_without_touching_the_network() {
        let app = App::new();
        // Torznab yapılandırılmamış; yine de hata değil, çünkü boş sorgu
        // indekse hiç gitmemeli.
        let value = search(&app, serde_json::json!({"query": "  ", "limit": 10}))
            .await
            .unwrap();
        assert_eq!(value["tracks"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn an_id_that_is_not_an_infohash_says_so_instead_of_starting_a_download() {
        let app = App::new();
        let error = resolve_source(&app, serde_json::json!({"id": "merhaba"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("infohash değil"), "{error}");
    }

    #[tokio::test]
    async fn a_file_index_that_is_not_a_number_is_refused_before_any_work() {
        let app = App::new();
        let hash = "e".repeat(40);
        let error = resolve_source(&app, serde_json::json!({"id": format!("{hash}/abc")}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("sayı değil"), "{error}");
    }

    #[tokio::test]
    async fn an_unknown_method_is_method_not_found_not_a_crash() {
        let mut app = App::new();
        let outcome = dispatch(&mut app, "teleport", serde_json::Value::Null)
            .await
            .unwrap();
        assert!(outcome.is_none());
    }
}
