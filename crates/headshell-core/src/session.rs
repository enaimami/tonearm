//! Çekirdeğin dış yüzeyi.
//!
//! CLI, GUI ve mobil bağlamalar **yalnızca** buradaki yöntemleri çağırır:
//! her komut tek bir çağrıdır, tanı kaydı ve kalıcılık burada halledilir.
//! Bir yeteneği CLI'den silsen çekirdek onu hâlâ sunar — Altın Kural'ın testi.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::diag::{DiagReport, Recorder, Stage};
use crate::error::{Error, ErrorKind, Result};
use crate::identity::{
    FingerprintLookup, MetadataLookup, OfflineLookup, Resolution, ResolveSummary, Resolver,
};
use crate::ids::ProviderId;
use crate::import::{self, ImportSummary};
use crate::library::{
    CatalogStore, CatalogTrack, CatalogWriteSummary, ListenStore, SearchHit, SqliteLibrary,
    WriteSummary,
};
use crate::model::{Listen, PlayRule, TrackRef};
use crate::net::HttpClient;
use crate::playback::QueueItem;
use crate::plugin::artifact::{ArtifactStore, InstallOutcome};
use crate::plugin::catalog::{
    self, CatalogFetch, CatalogPlugin, CatalogSummary, IndexedPlugin, Removed, UpdateOutcome,
    UpdateSummary,
};
use crate::plugin::consent::{ConsentStatus, ConsentStore};
use crate::plugin::manifest::{Permissions, PluginManifest};
use crate::plugin::{PluginEntry, PluginSummary};
use crate::provider::remote::{self, NewServer, RemoteServer, ServerKind, StoredAuth};
use crate::provider::{ProviderHealth, ProviderInfo, ProviderRegistry, ScanSummary};
use crate::secrets::Secrets;
use crate::sleeve::{self, CardSize, SleeveData};
use crate::stats::{self, StatsQuery, StatsReport};

/// Bir içe aktarma komutunun tam sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    pub import: ImportSummary,
    pub identity: ResolveSummary,
    pub write: WriteSummary,
    pub diag: DiagReport,
}

/// Tek parça çözümlemesinin sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolveReport {
    pub query: String,
    pub artist: String,
    pub title: String,
    pub resolution: Resolution,
    pub diag: DiagReport,
}

/// Arama sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchReport {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub diag: DiagReport,
}

/// İstatistik sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsResponse {
    pub report: StatsReport,
    pub diag: DiagReport,
}

/// Sleeve kartı sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SleeveResponse {
    pub data: SleeveData,
    pub size: CardSize,
    /// Dosya yazıldıysa biçim, yol ve bayt sayısı.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub written: Option<WrittenCard>,
    pub diag: DiagReport,
}

/// Yazılan kart dosyasının bilgisi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WrittenCard {
    pub path: std::path::PathBuf,
    pub bytes: u64,
    pub kind: sleeve::CardFileKind,
}

/// Kayıtlı sağlayıcıların listesi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderListReport {
    pub providers: Vec<ProviderInfo>,
    pub diag: DiagReport,
}

/// Bir sağlayıcının sınama sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderTestReport {
    pub info: ProviderInfo,
    pub health: ProviderHealth,
    pub diag: DiagReport,
}

/// Kurulu eklentilerin listesi (Faz 2 §2.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginListReport {
    pub plugins: Vec<PluginEntry>,
    pub summary: PluginSummary,
    /// İzinler zorlanıyor mu — **hayır** (D-040), ve bu her çıktıda yazar.
    /// Olmayan bir korumaya güvendirmemek için alan sabit değil, görünür.
    pub permissions_enforced: bool,
    pub diag: DiagReport,
}

/// Bir onay komutunun sonucu (`approve`, `disable`, `enable`, `forget`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginConsentReport {
    pub name: String,
    /// Hangi komut çalıştı.
    pub action: String,
    /// Eklentinin **beyan ettiği** izinler.
    pub permissions: Permissions,
    /// Motorun onun için indireceği eserler (D-055, D-069).
    ///
    /// `permissions.net`'ten **ayrı** duruyor ve bu bilerek: indirmeyi
    /// eklenti değil motor yapar. Aynı listeye karışsaydı kullanıcı "bu
    /// eklenti github.com'a bağlanıyor" diye okurdu — bağlanan motor, ve
    /// indirdiği şey karmasıyla sabitli.
    #[serde(default)]
    pub requires: Vec<crate::plugin::manifest::Requirement>,
    /// Eserlerin hangi platform için çözüldüğü — onay ekranı bu platformun
    /// yayınını gösterir.
    #[serde(default)]
    pub platform: String,
    /// Komuttan sonraki durum.
    pub status: ConsentStatus,
    pub permissions_enforced: bool,
    pub diag: DiagReport,
}

/// `headshell plugin install` çıktısı (D-055, D-069, D-071).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginInstallReport {
    /// Eklenti artık çalışabilir mi: beyan edilen eserlerin **hepsi** hazır mı.
    pub ready: bool,
    /// Manifestin beyan ettiği eser sayısı. `0` geçerli bir cevap: eklenti
    /// hiçbir şey istemiyor demek, "bakmadım" demek değil (K9).
    pub declared: usize,
    /// Eserlerin çözüldüğü platform (`linux-x86_64`). api 1'de burada
    /// motorun bulduğu Python yazıyordu; api 2'de yorumlayıcı gömülü ve
    /// seçilecek tek şey platformun ikilisi.
    pub platform: String,
    /// Eklenti bu komutta katalogdan indirildiyse ne indirildiği (D-071).
    /// `None`: eklenti zaten diskteydi ve katalog **okunmadı**.
    #[serde(default)]
    pub fetched: Option<CatalogFetch>,
    /// Eklentinin beyan ettiği izinler — onaya sunulacak olan.
    #[serde(default)]
    pub permissions: Permissions,
    /// Kurulumdan sonraki onay durumu. Katalogdan gelmek onay değildir
    /// (D-040): yeni kurulan eklenti `not_asked` der.
    pub consent: ConsentStatus,
    pub report: crate::plugin::artifact::InstallReport,
    pub diag: DiagReport,
}

/// `headshell plugin catalog` çıktısı (D-071).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginCatalogReport {
    /// Okunan katalog.
    pub index: String,
    /// Araç durumunun ölçüldüğü platform.
    pub platform: String,
    pub plugins: Vec<CatalogPlugin>,
    /// **Bu katalogdan** kurulmuş ama artık listede olmayan eklentiler.
    pub delisted: Vec<String>,
    pub summary: CatalogSummary,
    pub diag: DiagReport,
}

/// Bir eklentinin güncelleme sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginUpdate {
    pub name: String,
    pub outcome: UpdateOutcome,
    /// Güncellenen sürümün araçlarının kurulumu (D-055). Yalnızca
    /// `updated`'da dolu.
    #[serde(default)]
    pub tools: Vec<(String, InstallOutcome)>,
    /// Araçlar kurulamadıysa sebebi. Eklentinin dosyaları yine de güncellendi
    /// ve bu ayrı söyleniyor: "güncellendi" ile "çalışır" aynı şey değil.
    pub tools_error: Option<String>,
    /// Güncellemeden sonraki onay durumu; izinler büyüdüyse
    /// `needs_approval` (D-040).
    pub consent: Option<ConsentStatus>,
}

/// `headshell plugin update` çıktısı (D-071).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginUpdateReport {
    pub index: String,
    pub platform: String,
    pub plugins: Vec<PluginUpdate>,
    pub summary: UpdateSummary,
    pub diag: DiagReport,
}

/// `headshell plugin remove` çıktısı (D-071).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRemoveReport {
    pub name: String,
    pub removed: Removed,
    /// Onay kaydı vardı ve unutuldu: aynı adla yeniden kurulan bir eklenti
    /// baştan sorulur — başka bir eklenti eskisinin onayını devralmasın.
    pub consent_forgotten: bool,
    /// Eklentinin ad alanında **kalan** sırların anahtar adları (D-042).
    /// Silinmedi: kullanıcının girdiği bir değer (çerez, anahtar) sessizce
    /// gitmemeli. Değerler bu listede yok.
    pub kept_secrets: Vec<String>,
    pub diag: DiagReport,
}

/// `headshell plugin index` çıktısı — katalog deposunun bakımı (D-071).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginIndexReport {
    /// İndeks dosyası.
    pub path: std::path::PathBuf,
    pub url_template: String,
    pub plugins: Vec<IndexedPlugin>,
    /// Diskteki indeks üretilenle zaten aynı mıydı.
    pub up_to_date: bool,
    /// Bu komut dosyayı yazdı mı (`--check` hiç yazmaz).
    pub written: bool,
    pub diag: DiagReport,
}

/// Sır deposunun **anahtar adları** (değerler yok, D-042).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretListReport {
    pub namespaces: std::collections::BTreeMap<String, Vec<String>>,
    pub diag: DiagReport,
}

/// Bir sır yazma/silme komutunun sonucu. Değer **taşımaz**.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretWriteReport {
    pub namespace: String,
    pub key: String,
    pub action: String,
    pub changed: bool,
    pub diag: DiagReport,
}

/// Tarama sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanReport {
    /// Taranan kök dizinler. Boşsa kullanıcı `HEADSHELL_MUSIC_DIRS` vermemiş.
    pub dirs: Vec<std::path::PathBuf>,
    pub summary: ScanSummary,
    /// Kalıcı kataloğa ne yazıldığı: eklenen, güncellenen, düşen satırlar.
    pub write: CatalogWriteSummary,
    /// Tarama gerçekten koştu mu. `--if-stale` atlamış olabilir (D-025).
    pub scanned: bool,
    /// Neden koştu ya da neden atlandı — tanıya ait (K9): "değişmedi" ile
    /// "bakamadım" farklı şeylerdir.
    pub reason: String,
    pub diag: DiagReport,
}

/// Kayıtlı bir uzak sunucunun **sırsız** özeti.
///
/// Token ve API anahtarı bu tipte **yok**: `--json` çıktısı boru hattına,
/// log'a ya da hata raporuna girebilir. Kimlik bilgisi yalnızca `0600`
/// izinli `servers.json`'da durur (D-021).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSummary {
    pub id: ProviderId,
    pub kind: ServerKind,
    pub url: String,
    pub username: String,
    /// Hangi kimlik yolu: `subsonic_token` ya da `api_key`.
    pub auth: String,
}

impl ServerSummary {
    fn of(server: &RemoteServer) -> Self {
        Self {
            id: server.id.clone(),
            kind: server.kind,
            url: server.url.clone(),
            username: server.username.clone(),
            auth: match server.auth {
                StoredAuth::SubsonicToken { .. } => "subsonic_token".to_owned(),
                StoredAuth::ApiKey { .. } => "api_key".to_owned(),
            },
        }
    }
}

/// Kayıtlı sunucuların listesi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerListReport {
    pub servers: Vec<ServerSummary>,
    /// Kayıt dosyasının yolu — "nereye yazıldı?" sorusu tanıya ait.
    pub path: std::path::PathBuf,
    pub diag: DiagReport,
}

/// Sunucu ekleme sonucu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerAddReport {
    pub server: ServerSummary,
    /// Sunucuya bağlanıp kimlik doğrulandı mı.
    pub verified: bool,
    /// Kayıt sırasında oluşan gözlemler (zayıf entropi, öğrenilemeyen alan…).
    pub notes: Vec<String>,
    pub diag: DiagReport,
}

/// Sunucu silme sonucu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRemoveReport {
    pub id: ProviderId,
    pub removed: bool,
    pub remaining: usize,
    pub diag: DiagReport,
}

/// Çalma seçenekleri.
///
/// Ayrı bir struct: `uniffi` için de tek bir record olarak geçer ve yeni
/// seçenek eklemek çağıranların imzasını kırmaz.
///
/// `query` ödünç değil **sahipli** bir `String`: K7 dışa açılan tiplerde
/// lifetime yasaklıyor ve `uniffi` bir record alanında `&str`'i ifade
/// edemiyor. Kopyanın bedeli komut başına tek bir kısa metin.
#[derive(Debug, Clone)]
pub struct PlayOptions {
    /// Aranacak metin.
    pub query: String,
    /// Eşleşen tüm parçalar kuyruğa alınsın mı (false: yalnızca ilki).
    pub all: bool,
    /// Kuyruk karıştırılsın mı.
    pub shuffle: bool,
    /// Çalmadan yalnızca kuyruğu göster.
    pub dry_run: bool,
    /// Aramadan en fazla kaç sonuç alınacağı.
    pub limit: usize,
}

impl PlayOptions {
    /// Varsayılan seçeneklerle: ilk eşleşmeyi çal.
    #[must_use]
    pub fn new(query: String) -> Self {
        Self {
            query,
            all: false,
            shuffle: false,
            dry_run: false,
            limit: 100,
        }
    }
}

/// Bir çalma komutunun sonucu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayReport {
    pub query: String,
    /// Kuyruğa alınan parçalar.
    pub queued: Vec<QueueItem>,
    /// Çalma sonunda üretilen dinleme kayıtları (§1.6).
    pub listens_recorded: usize,
    /// Gerçekten çalındı mı (`--dry-run` ile false).
    pub played: bool,
    pub diag: DiagReport,
}

/// Açık bir kütüphane üzerinde çalışan oturum.
pub struct Session {
    config: Config,
    library: SqliteLibrary,
}

impl Session {
    /// Yapılandırmadaki kütüphaneyi açar (yoksa oluşturur).
    ///
    /// # Errors
    /// Veri dizini oluşturulamazsa ya da veritabanı açılamazsa.
    pub fn open(config: Config) -> Result<Self> {
        config.ensure_data_dir()?;
        let library = SqliteLibrary::open(config.database_path())?;
        Ok(Self { config, library })
    }

    /// Kullanılan yapılandırma.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Bir export arşivini içe aktarır, kimlikleri çözer ve kütüphaneye yazar.
    ///
    /// Üstveri kaynağı çağıran tarafından verilir; Faz 0'da
    /// [`OfflineLookup`] ile çağrılır, ağ geldiğinde imza değişmez.
    /// Generic değil `Arc<dyn _>` — K7 (`uniffi` generic ifade edemez).
    ///
    /// # Errors
    /// Arşiv okunamaz/tanınmazsa, çözümleme kaynağı hata verirse ya da
    /// yazma başarısız olursa. Hata hangi aşamada olduğunu taşır.
    pub async fn import_archive(
        &mut self,
        path: &Path,
        lookup: Arc<dyn MetadataLookup>,
    ) -> Result<ImportReport> {
        let mut rec = Recorder::start(
            format!("import {}", path.display()),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = self.import_inner(path, lookup, &mut rec).await;
        self.finish(rec, result, |(import, identity, write), diag| {
            ImportReport {
                import,
                identity,
                write,
                diag,
            }
        })
    }

    async fn import_inner(
        &mut self,
        path: &Path,
        lookup: Arc<dyn MetadataLookup>,
        rec: &mut Recorder,
    ) -> Result<(ImportSummary, ResolveSummary, WriteSummary)> {
        let mut archive = open_archive(path)?;
        let outcome = import::import(archive.as_mut())?;
        outcome.summary.record_into(rec);

        let tracks: Vec<TrackRef> = outcome
            .listens
            .iter()
            .map(|listen| listen.track.clone())
            .collect();
        let resolver = Resolver::new(lookup);
        let (resolutions, identity) = resolver.resolve_all(&tracks).await?;
        identity.record_into(rec);

        let mut listens = outcome.listens;
        for (listen, resolution) in listens.iter_mut().zip(&resolutions) {
            listen.canonical_id = Some(resolution.canonical_id.clone());
        }

        let write = self.library.insert_listens(&listens)?;
        write.record_into(rec);

        // Çözümleme sonuçlarını parça satırlarına da yaz ki `search` ve
        // sonraki çalıştırmalar hangi yöntemin çözdüğünü bilsin.
        let mut seen = std::collections::HashSet::new();
        for (listen, resolution) in listens.iter().zip(&resolutions) {
            let key =
                crate::identity::normalize::track_key(&listen.track.artist, &listen.track.title);
            if seen.insert(key.clone()) {
                self.library.set_resolution(&key, resolution)?;
            }
        }

        Ok((outcome.summary, identity, write))
    }

    /// Tek bir `"Sanatçı - Başlık"` sorgusunu kimlik zincirinden geçirir.
    ///
    /// # Errors
    /// Sorgu biçimsizse ya da üstveri kaynağı hata verirse.
    pub async fn resolve_track(
        &self,
        query: &str,
        lookup: Arc<dyn MetadataLookup>,
    ) -> Result<ResolveReport> {
        let mut rec = Recorder::start(
            format!("resolve {query:?}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = async {
            let track = TrackRef::parse_query(query)?;
            let resolution = Resolver::new(lookup).resolve(&track).await?;
            rec.set(
                "identity.confidence_pct",
                (resolution.confidence * 100.0) as i64,
            );
            // Beraberlik tanının parçası: "%97 güven" ile "%97 güven, 10 aday
            // berabere" aynı çalıştırmayı anlatmıyor (K9).
            rec.set(
                "identity.tied_candidates",
                i64::try_from(resolution.tied_candidates).unwrap_or(i64::MAX),
            );
            rec.note(format!("yöntem: {}", resolution.method));
            Ok((track, resolution))
        }
        .await;

        let query = query.to_owned();
        self.finish(rec, result, move |(track, resolution), diag| {
            ResolveReport {
                query,
                artist: track.artist,
                title: track.title,
                resolution,
                diag,
            }
        })
    }

    /// Bir **ses dosyasını** kimlik zincirinden geçirir.
    ///
    /// [`Self::resolve_track`]'ten farkı, zincirin 4. halkasının da
    /// çalışabilmesi: üstveri dosyanın kendi etiketlerinden okunur ve metin
    /// halkaları sonuçsuz kalırsa parmak izi sorulur.
    ///
    /// `fingerprint_lookup` `None` ise zincir üç halkayla biter — bu
    /// **bir kusur değil bir yapılandırma**: kullanıcı ağa çıkmayı istemediyse
    /// AcoustID'ye de sorulmaz.
    ///
    /// # Errors
    /// Dosya okunamazsa ya da bir kaynak hata verirse.
    pub async fn resolve_file(
        &self,
        path: &Path,
        lookup: Arc<dyn MetadataLookup>,
        fingerprint_lookup: Option<Arc<dyn FingerprintLookup>>,
    ) -> Result<ResolveReport> {
        let label = path.display().to_string();
        let mut rec = Recorder::start(
            format!("resolve --file {label:?}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        // Halkanın bağlı olup olmadığı tanının parçası: parmak izi kaynağı
        // yokken "eşleşme bulunamadı" ile kaynak varken bulunamaması aynı
        // çalıştırma değil (K9).
        rec.set(
            "identity.fingerprint_lookup",
            i64::from(fingerprint_lookup.is_some()),
        );

        let result = async {
            let mut resolver = Resolver::new(lookup);
            if let Some(fingerprint_lookup) = fingerprint_lookup {
                resolver = resolver.with_fingerprint_lookup(fingerprint_lookup);
            }
            let (track, _) = crate::provider::local::read_track(path)?;
            let resolution = resolver.resolve_file(path).await?;
            rec.set(
                "identity.confidence_pct",
                (resolution.confidence * 100.0) as i64,
            );
            rec.set(
                "identity.tied_candidates",
                i64::try_from(resolution.tied_candidates).unwrap_or(i64::MAX),
            );
            rec.note(format!("yöntem: {}", resolution.method));
            Ok((track, resolution))
        }
        .await;

        self.finish(rec, result, move |(track, resolution), diag| {
            ResolveReport {
                query: label,
                artist: track.artist,
                title: track.title,
                resolution,
                diag,
            }
        })
    }

    /// Bu kurulumun AcoustID kaynağı — anahtar sır deposundan okunur.
    ///
    /// `None` dönmesi "ağ istenmedi" demek. Anahtarın **bulunamaması** ise
    /// `None` değil: kaynak yine kurulur ve ilk çağrıda ne yapılacağını
    /// söyleyen bir hata döner (K9 — "istemedim" ile "yapamıyorum" ayrı).
    ///
    /// # Errors
    /// Sır deposu okunamazsa ya da bu derlemede HTTP istemcisi yoksa.
    #[cfg(feature = "fingerprint")]
    pub fn fingerprint_lookup_for(
        &self,
        mode: LookupMode,
    ) -> Result<Option<Arc<dyn FingerprintLookup>>> {
        use crate::identity::acoustid::{AcoustIdLookup, SECRET_KEY, SECRET_NAMESPACE};

        if mode == LookupMode::Offline {
            return Ok(None);
        }
        let secrets = Secrets::load(&self.config.secrets_path())?;
        let key = secrets
            .namespace(SECRET_NAMESPACE)
            .get(SECRET_KEY)
            .cloned()
            .unwrap_or_default();
        let lookup = AcoustIdLookup::new(crate::net::default_http_client()?).with_api_key(key);
        Ok(Some(Arc::new(lookup)))
    }

    /// Bu kurulumun AcoustID kaynağı.
    ///
    /// Bu derlemede `fingerprint` feature'ı kapalı: zincirin 4. halkası yok
    /// ve `None` bunu anlatıyor. Çağıran sessizce "eşleşme yok" görmesin diye
    /// [`crate::identity::fingerprint::fingerprint_file`] aynı durumu ayrıca
    /// hata olarak da söylüyor.
    ///
    /// # Errors
    /// Bu derlemede hiç hata dönmez; imza açık derlemeyle aynı kalsın diye
    /// `Result`.
    #[cfg(not(feature = "fingerprint"))]
    pub fn fingerprint_lookup_for(
        &self,
        _mode: LookupMode,
    ) -> Result<Option<Arc<dyn FingerprintLookup>>> {
        Ok(None)
    }

    /// Kütüphanede tam metin arama.
    ///
    /// Gösterilen çalma sayısı `rule`'u geçen dinlemelerdir — `stats` ile
    /// birebir aynı hesap (D-008). Ham olay sayısı yalnızca tanı kaydına
    /// (`search.listen_events`) yazılır.
    ///
    /// # Errors
    /// Sorgu boşsa ya da veritabanı hatasında.
    pub fn search(&self, query: &str, limit: usize, rule: PlayRule) -> Result<SearchReport> {
        let mut rec = Recorder::start(
            format!("library search {query:?}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        let result = self.library.search(query, limit, rule).map(|outcome| {
            rec.set("search.hits", n(outcome.hits.len()));
            rec.set(
                "search.play_count",
                n(outcome.hits.iter().map(|hit| hit.play_count).sum()),
            );
            rec.set("search.listen_events", n(outcome.listen_events));
            outcome.hits
        });
        let query = query.to_owned();
        self.finish(rec, result, move |hits, diag| SearchReport {
            query,
            hits,
            diag,
        })
    }

    /// Kütüphanedeki dinlemelerden istatistik üretir.
    ///
    /// # Errors
    /// Kütüphane okunamazsa.
    pub fn stats(&self, query: StatsQuery) -> Result<StatsResponse> {
        let mut rec = Recorder::start(
            "stats".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = self.library.all_listens().map(|listens| {
            let report = stats::compute(&listens, query);
            report.record_into(&mut rec);
            report
        });
        self.finish(rec, result, |report, diag| StatsResponse { report, diag })
    }

    /// Paylaşılabilir Sleeve kartı üretir ve isteğe bağlı olarak dosyaya yazar.
    ///
    /// `out` verilirse uzantıya göre SVG veya PNG yazar; verilmezse yalnızca
    /// veri döner (JSON çıktısı veya başka bir tüketici için).
    ///
    /// # Errors
    /// Kütüphane okunamazsa, rasterizasyon veya dosya yazma başarısız olursa.
    pub fn sleeve(
        &self,
        query: StatsQuery,
        size: CardSize,
        out: Option<&Path>,
    ) -> Result<SleeveResponse> {
        let mut rec = Recorder::start(
            format!(
                "sleeve{}",
                query.year.map_or(String::new(), |y| format!(" {y}"))
            ),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = self.library.all_listens().and_then(|listens| {
            let report = stats::compute(&listens, query);
            report.record_into(&mut rec);
            let data = sleeve::card_data(&report, &listens);
            rec.set(
                "sleeve.hours",
                i64::try_from(data.plays).unwrap_or(i64::MAX),
            );
            rec.set(
                "sleeve.discoveries",
                i64::try_from(data.discoveries.len()).unwrap_or(i64::MAX),
            );
            rec.set(
                "sleeve.timeline_years",
                i64::try_from(data.by_year.len()).unwrap_or(i64::MAX),
            );

            let written = match out {
                Some(path) => {
                    let (kind, bytes) = sleeve::write_card(&data, size, path)?;
                    rec.set(
                        "sleeve.bytes_written",
                        i64::try_from(bytes).unwrap_or(i64::MAX),
                    );
                    rec.note(format!("çıktı: {}", path.display()));
                    Some(WrittenCard {
                        path: path.to_owned(),
                        bytes,
                        kind,
                    })
                }
                None => None,
            };

            Ok((data, written))
        });

        self.finish(rec, result, |(data, written), diag| SleeveResponse {
            data,
            size,
            written,
            diag,
        })
    }

    /// Sağlayıcıları listeler.
    ///
    /// # Errors
    /// Şu an hata üretmiyor; imza sağlayıcılar ağa taşındığında (Faz 2)
    /// değişmesin diye `Result`.
    pub fn providers(&self, registry: &ProviderRegistry) -> Result<ProviderListReport> {
        let mut rec = Recorder::start(
            "provider list".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        rec.set(
            "provider.count",
            i64::try_from(registry.len()).unwrap_or(i64::MAX),
        );
        let result = Ok(registry.list());
        self.finish(rec, result, |providers, diag| ProviderListReport {
            providers,
            diag,
        })
    }

    /// Bir sağlayıcıyı sınar: ayakta mı, kaç parça görüyor.
    ///
    /// # Errors
    /// Sağlayıcı kayıtlı değilse ya da sağlık sorgusu hata verirse.
    pub async fn test_provider(
        &self,
        registry: &ProviderRegistry,
        id: &ProviderId,
    ) -> Result<ProviderTestReport> {
        let mut rec = Recorder::start(
            format!("provider test {id}"),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = async {
            let provider = registry.get(id).ok_or_else(|| {
                Error::new(
                    Stage::ProviderCall,
                    ErrorKind::NotFound {
                        what: format!(
                            "sağlayıcı: {id} (kayıtlı olanlar: {})",
                            registry
                                .list()
                                .iter()
                                .map(|info| info.id.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    },
                )
            })?;
            let info = provider.info();
            let health = provider.health().await?;
            Ok((info, health))
        }
        .await;

        if let Ok((_, health)) = &result {
            rec.set("provider.reachable", i64::from(health.reachable));
            if let Some(count) = health.track_count {
                rec.set(
                    "provider.track_count",
                    i64::try_from(count).unwrap_or(i64::MAX),
                );
            }
            if let Some(detail) = &health.detail {
                rec.note(detail.clone());
            }
        }

        self.finish(rec, result, |(info, health), diag| ProviderTestReport {
            info,
            health,
            diag,
        })
    }

    /// Bir uzak sunucu kaydeder (§1.3, D-019/D-021).
    ///
    /// Parola **saklanmaz**: Subsonic'te salt/token türetilir, Jellyfin'de
    /// erişim anahtarına çevrilir. `spec.verify` açıksa kaydetmeden önce
    /// sunucuya bağlanılır — yanlış parolayı bir hafta sonra "çalmıyor"
    /// diye keşfetmektense şimdi söylemek iyidir.
    ///
    /// # Errors
    /// Aynı adla kayıt varsa, adres/kimlik eksikse, doğrulama başarısızsa
    /// ya da kayıt dosyası yazılamazsa.
    pub async fn add_server(
        &self,
        spec: NewServer,
        http: Arc<dyn crate::net::HttpClient>,
    ) -> Result<ServerAddReport> {
        let mut rec = Recorder::start(
            format!("provider add {} {}", spec.kind, spec.id),
            Some(self.config.data_dir().to_path_buf()),
        );

        let path = self.config.servers_path();
        let verify = spec.verify;
        let result = async {
            let mut servers = remote::load_servers(&path)?;
            if spec.id.as_str() == "local" {
                return Err(Error::new(
                    Stage::ConfigLoad,
                    ErrorKind::InvalidInput {
                        detail: "`local` adı yerel dosya sağlayıcısına ait".to_owned(),
                    },
                ));
            }
            if servers.iter().any(|existing| existing.id == spec.id) {
                // Üzerine sessizce yazmak, kullanıcının çalışan kaydını
                // fark etmeden değiştirmek olurdu.
                return Err(Error::new(
                    Stage::ConfigLoad,
                    ErrorKind::InvalidInput {
                        detail: format!(
                            "{} adında bir sunucu zaten kayıtlı; önce `headshell provider remove {}`",
                            spec.id, spec.id
                        ),
                    },
                ));
            }

            let (server, notes) = remote::prepare_server(&spec, http).await?;
            let summary = ServerSummary::of(&server);
            servers.push(server);
            remote::save_servers(&path, &servers)?;
            Ok((summary, notes))
        }
        .await;

        if let Ok((_, notes)) = &result {
            rec.set("server.verified", i64::from(verify));
            for note in notes {
                rec.note(note.clone());
            }
        }

        self.finish(rec, result, move |(server, notes), diag| ServerAddReport {
            server,
            verified: verify,
            notes,
            diag,
        })
    }

    /// Kayıtlı uzak sunucuları listeler (kimlik bilgisi olmadan).
    ///
    /// # Errors
    /// Kayıt dosyası okunamaz ya da bozuksa.
    pub fn list_servers(&self) -> Result<ServerListReport> {
        let mut rec = Recorder::start(
            "provider servers".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        let path = self.config.servers_path();
        let result = remote::load_servers(&path).map(|servers| {
            rec.set(
                "server.count",
                i64::try_from(servers.len()).unwrap_or(i64::MAX),
            );
            servers.iter().map(ServerSummary::of).collect::<Vec<_>>()
        });
        self.finish(rec, result, move |servers, diag| ServerListReport {
            servers,
            path,
            diag,
        })
    }

    /// Bir uzak sunucu kaydını siler.
    ///
    /// Kayıtlı olmayan bir ad **hata**: "sildim" deyip hiçbir şey yapmamak
    /// yazım hatasını gizler.
    ///
    /// # Errors
    /// Ad kayıtlı değilse ya da dosya yazılamazsa.
    pub fn remove_server(&self, id: &ProviderId) -> Result<ServerRemoveReport> {
        let mut rec = Recorder::start(
            format!("provider remove {id}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let path = self.config.servers_path();
        let result = (|| {
            let mut servers = remote::load_servers(&path)?;
            let before = servers.len();
            servers.retain(|server| &server.id != id);
            if servers.len() == before {
                return Err(Error::new(
                    Stage::ConfigLoad,
                    ErrorKind::NotFound {
                        what: format!("kayıtlı sunucu: {id}"),
                    },
                ));
            }
            remote::save_servers(&path, &servers)?;
            rec.set(
                "server.remaining",
                i64::try_from(servers.len()).unwrap_or(i64::MAX),
            );
            Ok(servers.len())
        })();

        let id = id.clone();
        self.finish(rec, result, move |remaining, diag| ServerRemoveReport {
            id,
            removed: true,
            remaining,
            diag,
        })
    }

    /// Yerel müzik dizinlerini tarar ve indeksi tazeler.
    ///
    /// # Errors
    /// Kök dizin okunamazsa. Tek tek dosya hataları hata değildir; özette
    /// sayılır (K9).
    pub async fn scan_providers(&mut self, registry: &ProviderRegistry) -> Result<ScanReport> {
        let mut rec = Recorder::start(
            "provider scan".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = Self::scan_inner(&mut self.library, registry).await;

        if let Ok((summary, write)) = &result {
            summary.record_into(&mut rec);
            let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
            rec.set("catalog.inserted", n(write.inserted));
            rec.set("catalog.updated", n(write.updated));
            rec.set("catalog.removed", n(write.removed));
            rec.set("catalog.unchanged", n(write.unchanged));
        }
        let dirs = self.config.music_dirs();
        self.finish(rec, result, move |(summary, write), diag| ScanReport {
            dirs,
            summary,
            write,
            scanned: true,
            reason: "istendi".to_owned(),
            diag,
        })
    }

    /// Yalnızca **bayatsa** tarar (D-025).
    ///
    /// Sağlayıcıya ucuz bir soru soruyor: katalog son taramadan beri değişmiş
    /// olabilir mi? Yerel sağlayıcı dizin damgalarına bakıp cevaplıyor; tam
    /// tarama yapılmıyor. Cevap "bilmiyorum" ise **tarıyoruz** — bilmediğimiz
    /// için atlamak, kullanıcının eklediği dosyayı görünmez yapardı.
    ///
    /// Bu bir dizin izleme (watch) değil, tetiklenince bakan bir yoklama:
    /// `notify` bağımlılığı eklenmedi, davranış her platformda aynı.
    ///
    /// # Errors
    /// Bayatlık sorusu ya da tarama başarısız olursa.
    pub async fn scan_providers_if_stale(
        &mut self,
        registry: &ProviderRegistry,
    ) -> Result<ScanReport> {
        let mut rec = Recorder::start(
            "provider scan --if-stale".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );

        let mut reasons = Vec::new();
        let mut stale = false;
        for provider in registry.all() {
            let id = provider.info().id;
            let Some(last) = self.library.last_scanned_at_ms(&id)? else {
                reasons.push(format!("{id}: hiç taranmadı"));
                stale = true;
                continue;
            };
            match provider.catalog_changed_since(last).await? {
                Some(true) => {
                    reasons.push(format!("{id}: değişmiş"));
                    stale = true;
                }
                Some(false) => reasons.push(format!("{id}: değişmemiş")),
                // "Bilmiyorum" atlamak için yeterli değil.
                None => {
                    reasons.push(format!("{id}: bilinmiyor"));
                    stale = true;
                }
            }
        }
        let reason = reasons.join(", ");
        rec.note(format!("bayatlık: {reason}"));

        if !stale {
            rec.set("scan.skipped", 1);
            let dirs = self.config.music_dirs();
            return self.finish(rec, Ok(()), move |(), diag| ScanReport {
                dirs,
                summary: ScanSummary::default(),
                write: CatalogWriteSummary::default(),
                scanned: false,
                reason,
                diag,
            });
        }

        let result = Self::scan_inner(&mut self.library, registry).await;
        if let Ok((summary, write)) = &result {
            summary.record_into(&mut rec);
            let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
            rec.set("catalog.inserted", n(write.inserted));
            rec.set("catalog.updated", n(write.updated));
            rec.set("catalog.removed", n(write.removed));
            rec.set("catalog.unchanged", n(write.unchanged));
        }
        let dirs = self.config.music_dirs();
        self.finish(rec, result, move |(summary, write), diag| ScanReport {
            dirs,
            summary,
            write,
            scanned: true,
            reason,
            diag,
        })
    }

    /// Taramayı yürütür ve sonucu **kalıcı kataloğa** yazar.
    ///
    /// Kütüphane ödünç alma çakışmasını önlemek için `&mut SqliteLibrary`
    /// ayrı parametre; `self` üzerinden çağrılamıyordu.
    async fn scan_inner(
        library: &mut SqliteLibrary,
        registry: &ProviderRegistry,
    ) -> Result<(ScanSummary, CatalogWriteSummary)> {
        let mut total = ScanSummary::default();
        let mut write_total = CatalogWriteSummary::default();
        let mut scanned = 0usize;

        for provider in registry.all() {
            let info = provider.info();
            // Damgalar: değişmemiş dosyanın etiketi yeniden okunmasın.
            let known = library.catalog_stamps(&info.id)?;

            let Some(scan) = provider.scan_catalog(&known).await? else {
                continue;
            };
            scanned += 1;
            total.files_seen += scan.summary.files_seen;
            total.audio_files += scan.summary.audio_files;
            total.indexed += scan.summary.indexed;
            total.tag_fallback += scan.summary.tag_fallback;
            total.failed += scan.summary.failed;
            total.unreadable_dirs += scan.summary.unreadable_dirs;
            total.unchanged += scan.summary.unchanged;

            // Değişmemiş satırların üstverisi taramadan gelmez; katalogdaki
            // hâlini koruyoruz. Aksi halde her tarama onları silerdi.
            let mut rows = Vec::with_capacity(scan.tracks.len());
            for entry in scan.tracks {
                match entry.track {
                    Some(track) => rows.push(CatalogTrack {
                        id: entry.id,
                        track,
                        from_tags: entry.from_tags,
                        mtime_ms: entry.mtime_ms,
                    }),
                    None => {
                        if let Some(existing) = library.catalog_get(&entry.id)? {
                            rows.push(existing);
                        }
                    }
                }
            }

            let write = library.replace_catalog(&info.id, &rows)?;
            write_total.inserted += write.inserted;
            write_total.updated += write.updated;
            write_total.removed += write.removed;
            write_total.unchanged += write.unchanged;
        }

        if scanned == 0 {
            return Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::NotFound {
                    what: "taranabilir kataloğu olan sağlayıcı".to_owned(),
                },
            ));
        }
        Ok((total, write_total))
    }

    /// Aramayı çalınabilir bir kuyruğa çevirir.
    ///
    /// `all` false ise yalnızca ilk eşleşme alınır. Sonuç boşsa **hata**
    /// döner: "çaldım ama ses yok" durumundan iyidir.
    ///
    /// # Errors
    /// Sağlayıcı yoksa, arama hata verirse ya da hiç eşleşme yoksa.
    pub async fn queue_from_search(
        &self,
        registry: &ProviderRegistry,
        query: &str,
        all: bool,
        limit: usize,
    ) -> Result<Vec<QueueItem>> {
        // Önce **kalıcı katalog**: tarama bir kez yapılır, arama diske
        // gitmeden FTS ile cevaplanır. Sağlayıcıya sormak yalnızca katalog
        // boşsa gerekir (henüz taranmamış ya da uzak sağlayıcı).
        let mut items: Vec<QueueItem> = self
            .library
            .search_catalog(query, limit)?
            .into_iter()
            .filter(|hit| {
                // Katalogda duran ama artık çalınamayan sağlayıcıyı atla.
                registry.get(&hit.id.provider).is_some_and(|provider| {
                    provider
                        .info()
                        .capabilities
                        .contains(crate::provider::Capabilities::STREAM)
                })
            })
            .map(|hit| QueueItem {
                id: hit.id,
                track: hit.track,
            })
            .collect();

        if items.is_empty() {
            let streamers = registry.with_capability(crate::provider::Capabilities::STREAM);
            if streamers.is_empty() {
                return Err(Error::new(
                    Stage::PlaybackResolve,
                    ErrorKind::NotFound {
                        what: "ses akışı verebilen sağlayıcı".to_owned(),
                    },
                ));
            }
            for provider in streamers {
                let hits = provider.search(query, limit).await?;
                items.extend(hits.into_iter().map(|hit| QueueItem {
                    id: hit.id,
                    track: hit.track,
                }));
            }
        }

        if items.is_empty() {
            return Err(Error::new(
                Stage::PlaybackResolve,
                ErrorKind::NotFound {
                    what: format!(
                        "{query:?} ile eşleşen parça (indeks boşsa: `headshell provider scan`)"
                    ),
                },
            ));
        }
        if !all {
            items.truncate(1);
        }
        Ok(items)
    }

    /// Arar, kuyruğa alır, çalar ve dinleme kayıtlarını yazar.
    ///
    /// Çalma bitene kadar bekler — CLI'nin bir döngü yazmasına gerek kalmasın
    /// diye akışın tamamı burada (Altın Kural). GUI ileride bunun yerine
    /// [`Session::queue_from_search`] + kendi `Player`'ıyla kendi döngüsünü
    /// kurar; ikisi de aynı çekirdek parçalarını kullanır.
    ///
    /// # Errors
    /// Eşleşme yoksa, sağlayıcı çalamıyorsa ya da ses hattı kurulamazsa.
    pub async fn play(
        &mut self,
        registry: &ProviderRegistry,
        options: PlayOptions,
    ) -> Result<PlayReport> {
        let mut rec = Recorder::start(
            format!("play {:?}", options.query),
            Some(self.config.data_dir().to_path_buf()),
        );

        let result = async {
            let items = self
                .queue_from_search(registry, &options.query, options.all, options.limit)
                .await?;

            let mut player = crate::playback::Player::new(registry.clone());
            if options.shuffle {
                player.queue_mut().set_shuffle(true);
            }

            if options.dry_run {
                player.queue_mut().replace(items.clone());
                return Ok((items, Vec::new(), false));
            }

            player.play_items(items.clone()).await?;

            // Kuyruk bitene kadar sür. Yoklama aralığı çapadan bağımsız:
            // pozisyon tüketici tarafında hesaplanır (D-015), burada
            // yalnızca "parça bitti mi" sorulur.
            //
            // Uyku `std::thread::sleep`: çekirdek bir async çalışma zamanı
            // seçmez (PLAN konvansiyonu), `tokio::time` burada kullanılamaz.
            // Ses zaten kendi iş parçacığında çaldığı için bu bekleme sesi
            // kesmiyor; yalnızca bu çağrı bloklanıyor.
            loop {
                player.tick().await?;
                if player.state() == crate::playback::PlayState::Stopped {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            player.stop();

            let listens = player.take_listens();
            Ok((items, listens, true))
        }
        .await;

        let report = match result {
            Ok((items, listens, played)) => {
                // Scrobble'lar burada yazılır: import verisiyle aynı tabloya (§1.6).
                let written = self.record_listens(&listens)?;
                rec.set(
                    "play.queued",
                    i64::try_from(items.len()).unwrap_or(i64::MAX),
                );
                rec.set(
                    "play.listens_recorded",
                    i64::try_from(written.inserted).unwrap_or(i64::MAX),
                );
                Ok((items, written.inserted, played))
            }
            Err(err) => Err(err),
        };

        let query = options.query;
        self.finish(
            rec,
            report,
            move |(queued, listens_recorded, played), diag| PlayReport {
                query,
                queued,
                listens_recorded,
                played,
                diag,
            },
        )
    }

    /// Bir arama sonucundan hazır bir [`crate::playback::Player`] kurar.
    ///
    /// `Session::play`'den farkı: **beklemez**. Çağıran kendi döngüsünü
    /// yürütür (TUI çizim döngüsü, GUI zamanlayıcısı), `tick()` çağırır ve
    /// biten dinlemeleri [`Session::record_listens`] ile yazar.
    ///
    /// # Errors
    /// Eşleşme yoksa ya da ilk parça çalınamazsa.
    pub async fn player_from_search(
        &self,
        registry: &ProviderRegistry,
        options: PlayOptions,
    ) -> Result<crate::playback::Player> {
        let items = self
            .queue_from_search(registry, &options.query, options.all, options.limit)
            .await?;

        let mut player = crate::playback::Player::new(registry.clone());
        if options.shuffle {
            player.queue_mut().set_shuffle(true);
        }
        if options.dry_run {
            player.queue_mut().replace(items);
        } else {
            player.play_items(items).await?;
        }
        Ok(player)
    }

    /// Çalınan parçaların dinleme kayıtlarını kütüphaneye yazar (§1.6).
    ///
    /// Import verisiyle **aynı tabloya** yazılır: geçmiş ve bugün tek bir
    /// zaman çizelgesi olur.
    ///
    /// # Errors
    /// Yazma başarısız olursa.
    pub fn record_listens(&mut self, listens: &[Listen]) -> Result<WriteSummary> {
        if listens.is_empty() {
            return Ok(WriteSummary::default());
        }
        self.library.insert_listens(listens)
    }

    /// Kurulu eklentileri listeler (Faz 2 §2.1). **Süreç başlatmaz.**
    ///
    /// # Errors
    /// Eklenti dizini okunamaz ya da onay defteri bozuksa.
    pub fn plugins(&self) -> Result<PluginListReport> {
        let mut rec = Recorder::start(
            "plugin list".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = crate::plugin::discover(&self.config);
        if let Ok((entries, summary)) = &result {
            summary.record_into(&mut rec);
            for entry in entries.iter().filter(|entry| !entry.is_loadable()) {
                rec.note(format!("{}: {}", entry.name, entry.status_text()));
            }
        }
        self.finish(rec, result, |(plugins, summary), diag| PluginListReport {
            plugins,
            summary,
            permissions_enforced: crate::plugin::PERMISSIONS_ENFORCED,
            diag,
        })
    }

    /// Bir eklentiyi kurar (D-055, D-069, D-071).
    ///
    /// Eklenti diskte **yoksa** önce katalogdan indirilir
    /// ([`crate::plugin::catalog`]): dosyalar sha256 ile doğrulanır, inen
    /// manifest indeksin gösterdiğiyle karşılaştırılır ve köken kaydıyla
    /// birlikte yerine konur. Ardından — eklenti zaten diskteyse yalnızca
    /// bu — beyan ettiği araçlar motorla kurulur.
    ///
    /// **Ağa çıkar ve bunu `--online` beklemeden yapar.** Bayrak örtük ağ
    /// erişimini engellemek için var ("bir export'u içe aktarmak kimseyi
    /// sessizce ağa bağlamaz"); burada indirme komutun kendisidir, yan
    /// etkisi değil. Kullanıcı `install` yazdıysa indirilmesini istemiştir.
    ///
    /// Diskte zaten olan eklenti için katalog **okunmaz** — bu komut
    /// güncellemez ([`Self::update_plugins`]) — ve kurulu eserler için ağa
    /// hiç çıkılmaz. Kurulan eklenti onay bekler: katalogdan gelmek onay
    /// değildir (D-040).
    ///
    /// # Errors
    /// Ad geçersizse, eklenti ne diskte ne katalogda varsa, katalog
    /// okunamazsa, bir dosyanın karması tutmazsa, manifest bozuksa, HTTP
    /// istemcisi bu derlemede yoksa ya da dosyalar yazılamazsa. Bir **esere**
    /// ulaşamamak hata değil: [`InstallOutcome`] içinde raporlanır, çünkü
    /// "ulaşılamadı", "yetim" ve "karma tutmadı" ayrı tanılardır (K9).
    pub async fn install_plugin(
        &self,
        name: &str,
        http: Arc<dyn HttpClient>,
    ) -> Result<PluginInstallReport> {
        let mut rec = Recorder::start(
            format!("plugin install {name}"),
            Some(self.config.data_dir().to_path_buf()),
        );

        let store = ArtifactStore::new(&self.config);
        let result = async {
            crate::plugin::manifest::validate_local_name(name).map_err(|detail| {
                Error::new(Stage::PluginLoad, ErrorKind::InvalidInput { detail })
            })?;
            let dir = self.config.plugins_dir().join(name);
            let missing = matches!(
                std::fs::symlink_metadata(&dir),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound
            );
            let fetched = if missing {
                let index = self.config.plugin_index_url();
                let catalog = catalog::fetch(http.as_ref(), &index).await?;
                Some(catalog::install(&self.config, http.as_ref(), &catalog, name).await?)
            } else {
                None
            };

            let manifest = PluginManifest::load(&dir)?;
            let outcomes = install_artifacts(&store, &manifest)?;
            let consent = ConsentStore::load(&self.config.plugin_consent_path())?
                .status(name, &manifest.permissions);
            Ok((manifest, outcomes, fetched, consent))
        }
        .await;

        rec.note(format!("platform: {}", store.platform()));
        if let Ok((_, outcomes, fetched, consent)) = &result {
            match fetched {
                Some(fetched) => rec.note(format!(
                    "katalogdan indirildi: {name} {} ← {}",
                    fetched.version, fetched.index
                )),
                None => rec.note("eklenti zaten diskteydi; katalog okunmadı".to_owned()),
            }
            for (name, outcome) in outcomes {
                rec.note(format!("{name}: {}", outcome.describe()));
            }
            rec.note(format!("onay: {}", consent.describe()));
        }

        let plugin = name.to_owned();
        let platform = store.platform().to_owned();
        self.finish(
            rec,
            result,
            move |(manifest, outcomes, fetched, consent), diag| {
                let report = crate::plugin::artifact::InstallReport { plugin, outcomes };
                PluginInstallReport {
                    ready: report.is_ready(),
                    declared: manifest.requires.len(),
                    platform,
                    fetched,
                    permissions: manifest.permissions,
                    consent,
                    report,
                    diag,
                }
            },
        )
    }

    /// Eklenti kataloğunu okur ve bu makineye karşı gösterir (D-071): her
    /// eklenti kurulu mu, güncellemesi var mı, neden kurulamıyor.
    ///
    /// Ağa **yalnızca** kataloğu okumak için çıkar; hiçbir şey kurmaz. Adres
    /// [`Config::plugin_index_url`].
    ///
    /// # Errors
    /// Kataloğa ulaşılamazsa (`NETWORK_REQUEST`), indeks yoksa ya da
    /// okunamıyorsa (`PLUGIN_CATALOG`). Bozuk bir **girdi** hata değil:
    /// o girdinin `problem`'inde yazar.
    pub async fn plugin_catalog(&self, http: Arc<dyn HttpClient>) -> Result<PluginCatalogReport> {
        let mut rec = Recorder::start(
            "plugin catalog".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        let index = self.config.plugin_index_url();
        rec.note(format!("katalog: {index}"));

        let result = match catalog::fetch(http.as_ref(), &index).await {
            Ok(catalog) => catalog.survey(&self.config),
            Err(err) => Err(err),
        };
        if let Ok(survey) = &result {
            survey.summary.record_into(&mut rec);
            for plugin in &survey.plugins {
                if let Some(problem) = &plugin.problem {
                    rec.note(format!("{}: {problem}", plugin.name));
                }
            }
            for name in &survey.delisted {
                rec.note(format!(
                    "{name}: bu katalogdan kurulmuş ama artık listede yok"
                ));
            }
        }

        let platform = crate::plugin::artifact::current_platform();
        self.finish(rec, result, move |survey, diag| PluginCatalogReport {
            index,
            platform,
            plugins: survey.plugins,
            delisted: survey.delisted,
            summary: survey.summary,
            diag,
        })
    }

    /// Katalogdan kurulmuş eklentileri katalogdaki sürüme getirir (D-071).
    ///
    /// `name` verilirse yalnızca o eklenti, ve yapılamıyorsa (elle kurulmuş,
    /// yerelde değiştirilmiş, katalogda yok) **hata** — kullanıcı açıkça
    /// istedi. Verilmezse katalogdaki adlardan kurulu olanların hepsi; tek
    /// birinin düşmesi ötekileri durdurmaz, sonuçlar tek tek yazar (K9).
    ///
    /// Güncelleme **yalnızca** köken kaydı olan ve dosyaları kayıtla aynı
    /// olan eklentiye dokunur. Yeni sürümün araçları kurulur; araç
    /// değişikliği onay istemez ama raporda yazar. İzinler büyüdüyse eklenti
    /// yeniden onay bekler (D-040).
    ///
    /// # Errors
    /// Katalog okunamazsa; `name` verildiyse o eklenti güncellenemezse.
    pub async fn update_plugins(
        &self,
        name: Option<&str>,
        http: Arc<dyn HttpClient>,
    ) -> Result<PluginUpdateReport> {
        let command = match name {
            Some(name) => format!("plugin update {name}"),
            None => "plugin update".to_owned(),
        };
        let mut rec = Recorder::start(command, Some(self.config.data_dir().to_path_buf()));
        let index = self.config.plugin_index_url();
        let platform = crate::plugin::artifact::current_platform();
        let store = ArtifactStore::new(&self.config);
        rec.note(format!("katalog: {index}"));

        let result = async {
            let catalog = catalog::fetch(http.as_ref(), &index).await?;
            let names = match name {
                Some(name) => vec![name.to_owned()],
                None => catalog.update_candidates(&self.config)?,
            };
            let mut plugins = Vec::new();
            for plugin in names {
                let outcome =
                    catalog::update(&self.config, http.as_ref(), &catalog, &plugin, &platform)
                        .await;
                let outcome = match (outcome, name.is_some()) {
                    (Ok(UpdateOutcome::Skipped { reason }), true) => {
                        return Err(Error::new(
                            Stage::PluginCatalog,
                            ErrorKind::PluginCatalog {
                                index: index.clone(),
                                detail: format!("{plugin} güncellenmedi: {reason}"),
                            },
                        ));
                    }
                    (Err(err), true) => return Err(err),
                    (Err(err), false) => UpdateOutcome::Failed {
                        error: err.chain_text().replace('\n', " "),
                    },
                    (Ok(outcome), _) => outcome,
                };
                let mut update = PluginUpdate {
                    name: plugin.clone(),
                    outcome,
                    tools: Vec::new(),
                    tools_error: None,
                    consent: None,
                };
                if matches!(update.outcome, UpdateOutcome::Updated { .. }) {
                    let manifest = PluginManifest::load(&self.config.plugins_dir().join(&plugin))?;
                    match install_artifacts(&store, &manifest) {
                        Ok(tools) => update.tools = tools,
                        Err(err) if name.is_some() => return Err(err),
                        Err(err) => update.tools_error = Some(err.chain_text().replace('\n', " ")),
                    }
                    update.consent = Some(
                        ConsentStore::load(&self.config.plugin_consent_path())?
                            .status(&plugin, &manifest.permissions),
                    );
                }
                plugins.push(update);
            }
            Ok(plugins)
        }
        .await;

        if let Ok(plugins) = &result {
            UpdateSummary::of(plugins.iter().map(|plugin| &plugin.outcome)).record_into(&mut rec);
            for plugin in plugins {
                rec.note(format!("{}: {}", plugin.name, plugin.outcome.describe()));
                if let UpdateOutcome::Updated { tools_changed, .. } = &plugin.outcome {
                    for change in tools_changed {
                        rec.note(format!(
                            "{}: araç değişti — {} (onay istenmez, D-071)",
                            plugin.name,
                            change.describe()
                        ));
                    }
                }
            }
        }

        self.finish(rec, result, move |plugins, diag| PluginUpdateReport {
            index,
            platform,
            summary: UpdateSummary::of(plugins.iter().map(|plugin| &plugin.outcome)),
            plugins,
            diag,
        })
    }

    /// Bir eklentiyi kaldırır: dizinini (içindeki `state/` ile) siler ve
    /// onayını unutur (D-071).
    ///
    /// Onay **önce** unutulur: dizin silinemezse eklenti onaysız kalır — ters
    /// sıra, onaylı ama yarım silinmiş bir eklenti bırakabilirdi. Sırlar ve
    /// motorun kurduğu araçlar silinmez; kalan sırların adları raporda.
    /// Dizin bir sembolik bağlantıysa yalnızca bağlantı kaldırılır.
    ///
    /// # Errors
    /// Ad geçersizse, eklenti kurulu değilse, onay defteri ya da sır dosyası
    /// bozuksa, dizin silinemezse.
    pub fn remove_plugin(&self, name: &str) -> Result<PluginRemoveReport> {
        let mut rec = Recorder::start(
            format!("plugin remove {name}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = (|| {
            catalog::installed_dir(&self.config, name)?;
            let path = self.config.plugin_consent_path();
            let mut consents = ConsentStore::load(&path)?;
            let consent_forgotten = consents.forget(name);
            if consent_forgotten {
                consents.save(&path)?;
            }
            let removed = catalog::remove(&self.config, name)?;
            let kept_secrets: Vec<String> = Secrets::load(&self.config.secrets_path())?
                .namespace(&crate::secrets::plugin_namespace(name))
                .into_keys()
                .collect();
            Ok((removed, consent_forgotten, kept_secrets))
        })();

        if let Ok((removed, consent_forgotten, kept_secrets)) = &result {
            rec.note(format!("kaldırıldı: {}", removed.path.display()));
            if let Some(target) = &removed.link_target {
                rec.note(format!(
                    "bir bağlantıydı; hedefine dokunulmadı: {}",
                    target.display()
                ));
            }
            rec.note(format!(
                "onay: {}",
                if *consent_forgotten {
                    "unutuldu"
                } else {
                    "kaydı yoktu"
                }
            ));
            if !kept_secrets.is_empty() {
                rec.note(format!("kalan sırlar: {}", kept_secrets.join(", ")));
            }
        }

        let name = name.to_owned();
        self.finish(
            rec,
            result,
            move |(removed, consent_forgotten, kept_secrets), diag| PluginRemoveReport {
                name,
                removed,
                consent_forgotten,
                kept_secrets,
                diag,
            },
        )
    }

    /// Katalog deposunun indeksini üretir ya da denetler (D-071).
    ///
    /// Katalog **bakımı** için: `dir` bir `headshell/plugins` kopyası. Her
    /// eklenti çekirdeğin kendi manifest doğrulamasından geçer — kurulumda
    /// uygulanacak kuralın aynısı — ve dosyaların karması hesaplanır.
    /// `url_template` verilmezse var olan `index.json`'daki kullanılır, ki
    /// her üretim aynı adresleri yazsın.
    ///
    /// `check` açıkken hiçbir şey yazılmaz; indeks güncel değilse **neyin**
    /// farklı olduğunu söyleyen bir hata döner (katalog deposunun CI'ı).
    ///
    /// # Errors
    /// Şablon yoksa ya da geçersizse, bir eklenti geçersizse, `check`
    /// açıkken indeks güncel değilse, dosya okunamaz ya da yazılamazsa.
    pub fn build_plugin_index(
        &self,
        dir: &Path,
        url_template: Option<&str>,
        check: bool,
    ) -> Result<PluginIndexReport> {
        let mut rec = Recorder::start(
            format!(
                "plugin index {}{}",
                dir.display(),
                if check { " --check" } else { "" }
            ),
            Some(self.config.data_dir().to_path_buf()),
        );
        let index_path = dir.join(catalog::INDEX_FILE);
        let origin = index_path.display().to_string();
        let result = (|| {
            let template = match url_template {
                Some(template) => template.to_owned(),
                None => catalog::read_url_template(&index_path)?.ok_or_else(|| {
                    Error::new(
                        Stage::PluginCatalog,
                        ErrorKind::PluginCatalog {
                            index: origin.clone(),
                            detail: "adres şablonu yok: ilk üretimde `--url-template` verin; \
                                     sonrakiler index.json'daki şablonu kullanır"
                                .to_owned(),
                        },
                    )
                })?,
            };
            let built = catalog::build_index(dir, &template)?;
            let existing = match std::fs::read_to_string(&index_path) {
                Ok(text) => Some(text),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    return Err(crate::error::io_err(Stage::PluginCatalog, &index_path, err));
                }
            };
            let up_to_date = existing.as_deref() == Some(built.json.as_str());
            if check && !up_to_date {
                let differences = match &existing {
                    Some(old) => catalog::index_differences(old, &built.json),
                    None => vec!["index.json yok".to_owned()],
                };
                return Err(Error::new(
                    Stage::PluginCatalog,
                    ErrorKind::PluginCatalog {
                        index: origin.clone(),
                        detail: format!(
                            "index.json güncel değil — `headshell plugin index {}` ile yeniden \
                             üretin: {}",
                            dir.display(),
                            differences.join("; ")
                        ),
                    },
                ));
            }
            let written = !check && !up_to_date;
            if written {
                catalog::write_index(dir, &built.json)?;
            }
            Ok((template, built.plugins, up_to_date, written))
        })();

        if let Ok((_, plugins, up_to_date, written)) = &result {
            rec.set(
                "index.plugins",
                i64::try_from(plugins.len()).unwrap_or(i64::MAX),
            );
            for plugin in plugins {
                rec.note(format!("{} {}", plugin.name, plugin.version));
            }
            rec.note(if *written {
                "index.json yazıldı".to_owned()
            } else if *up_to_date {
                "index.json zaten güncel".to_owned()
            } else {
                "index.json yazılmadı".to_owned()
            });
        }

        self.finish(
            rec,
            result,
            move |(url_template, plugins, up_to_date, written), diag| PluginIndexReport {
                path: index_path,
                url_template,
                plugins,
                up_to_date,
                written,
                diag,
            },
        )
    }

    /// Bir eklentinin **beyan ettiği** izinleri onaylar (D-040).
    ///
    /// Onaylanan küme manifestten okunur: çağıran kendi izin listesini
    /// uyduramaz, kullanıcı yalnızca eklentinin istediğine evet der.
    ///
    /// # Errors
    /// Eklenti bulunamazsa, manifesti bozuksa ya da defter yazılamazsa.
    pub fn approve_plugin(&self, name: &str) -> Result<PluginConsentReport> {
        self.consent_command(name, "approve", |store, name, requested| {
            store.approve(name, requested, jiff::Timestamp::now());
            Ok(())
        })
    }

    /// Eklentiyi kapatır; onay kaydı korunur.
    ///
    /// # Errors
    /// Eklenti bulunamazsa ya da hiç onaylanmamışsa.
    pub fn disable_plugin(&self, name: &str) -> Result<PluginConsentReport> {
        self.consent_command(name, "disable", |store, name, _| {
            missing_consent(store.disable(name), name)
        })
    }

    /// Kapalı bir eklentiyi yeniden açar.
    ///
    /// # Errors
    /// Eklenti bulunamazsa ya da hiç onaylanmamışsa.
    pub fn enable_plugin(&self, name: &str) -> Result<PluginConsentReport> {
        self.consent_command(name, "enable", |store, name, _| {
            missing_consent(store.enable(name), name)
        })
    }

    /// Onayı tamamen unutur: eklenti bir dahaki sefere baştan sorulur.
    ///
    /// # Errors
    /// Eklenti bulunamazsa ya da hiç onaylanmamışsa.
    pub fn forget_plugin(&self, name: &str) -> Result<PluginConsentReport> {
        self.consent_command(name, "forget", |store, name, _| {
            missing_consent(store.forget(name), name)
        })
    }

    fn consent_command(
        &self,
        name: &str,
        action: &str,
        apply: impl FnOnce(&mut ConsentStore, &str, &Permissions) -> Result<()>,
    ) -> Result<PluginConsentReport> {
        let mut rec = Recorder::start(
            format!("plugin {action} {name}"),
            Some(self.config.data_dir().to_path_buf()),
        );

        let action = action.to_owned();
        let platform = crate::plugin::artifact::current_platform();
        let result = (|| {
            let dir = self.config.plugins_dir().join(name);
            let manifest = crate::plugin::manifest::PluginManifest::load(&dir)?;
            let path = self.config.plugin_consent_path();
            let mut store = ConsentStore::load(&path)?;
            apply(&mut store, name, &manifest.permissions)?;
            store.save(&path)?;
            let status = store.status(name, &manifest.permissions);
            Ok((manifest, status))
        })();

        if let Ok((manifest, status)) = &result {
            rec.note(format!(
                "beyan edilen izinler: {}",
                manifest.permissions.describe()
            ));
            for requirement in &manifest.requires {
                match requirement.asset_for(&platform) {
                    Some(asset) => rec.note(format!(
                        "motorun indireceği: {} {} ({platform}) ← {} (sha256 {})",
                        requirement.name, requirement.version, asset.url, asset.sha256
                    )),
                    None => rec.note(format!(
                        "{} {}: bu platform ({platform}) için yayın yok",
                        requirement.name, requirement.version
                    )),
                }
            }
            rec.note(format!("durum: {}", status.describe()));
        }

        self.finish(rec, result, move |(manifest, status), diag| {
            PluginConsentReport {
                name: manifest.name,
                action,
                permissions: manifest.permissions,
                requires: manifest.requires,
                platform,
                status,
                permissions_enforced: crate::plugin::PERMISSIONS_ENFORCED,
                diag,
            }
        })
    }

    /// Sır deposundaki **anahtar adlarını** listeler (D-042).
    ///
    /// Değerler dönmüyor: bu çıktı `--json` ile boru hattına ve tanı
    /// raporuna gidebilir.
    ///
    /// # Errors
    /// Sır dosyası bozuksa.
    pub fn secrets(&self) -> Result<SecretListReport> {
        let mut rec = Recorder::start(
            "secret list".to_owned(),
            Some(self.config.data_dir().to_path_buf()),
        );
        let result = Secrets::load(&self.config.secrets_path()).map(|secrets| secrets.describe());
        if let Ok(namespaces) = &result {
            rec.set(
                "secrets.namespaces",
                i64::try_from(namespaces.len()).unwrap_or(i64::MAX),
            );
        }
        self.finish(rec, result, |namespaces, diag| SecretListReport {
            namespaces,
            diag,
        })
    }

    /// Bir sır yazar.
    ///
    /// # Errors
    /// Sır dosyası okunamaz ya da yazılamazsa.
    pub fn set_secret(&self, namespace: &str, key: &str, value: &str) -> Result<SecretWriteReport> {
        self.secret_command(namespace, key, "set", |secrets| {
            secrets.set(namespace, key, value);
            Ok(true)
        })
    }

    /// Bir sırrı siler.
    ///
    /// # Errors
    /// Sır dosyası okunamaz/yazılamazsa ya da anahtar yoksa.
    pub fn remove_secret(&self, namespace: &str, key: &str) -> Result<SecretWriteReport> {
        self.secret_command(namespace, key, "remove", |secrets| {
            if secrets.remove(namespace, key) {
                Ok(true)
            } else {
                Err(Error::new(
                    Stage::ConfigLoad,
                    ErrorKind::NotFound {
                        what: format!("sır: {namespace} / {key}"),
                    },
                ))
            }
        })
    }

    fn secret_command(
        &self,
        namespace: &str,
        key: &str,
        action: &str,
        apply: impl FnOnce(&mut Secrets) -> Result<bool>,
    ) -> Result<SecretWriteReport> {
        // Komut satırı tanıya yazılıyor; **değer buraya girmiyor** (D-042).
        let rec = Recorder::start(
            format!("secret {action} {namespace} {key}"),
            Some(self.config.data_dir().to_path_buf()),
        );
        let namespace = namespace.to_owned();
        let key = key.to_owned();
        let action = action.to_owned();

        let path = self.config.secrets_path();
        let result = (|| {
            let mut secrets = Secrets::load(&path)?;
            let changed = apply(&mut secrets)?;
            secrets.save(&path)?;
            Ok(changed)
        })();

        self.finish(rec, result, move |changed, diag| SecretWriteReport {
            namespace,
            key,
            action,
            changed,
            diag,
        })
    }

    /// Son çalıştırmanın tanı raporu.
    ///
    /// # Errors
    /// Rapor dosyası bozuksa.
    pub fn last_diag(&self) -> Result<Option<DiagReport>> {
        crate::diag::load_last_run(&self.config.last_run_path())
    }

    /// Komutu kapatır: tanı raporunu diske yazar, sonucu paketler.
    ///
    /// Rapor hem başarıda hem başarısızlıkta yazılır — `headshell diag` en çok
    /// bir şey patladığında lazım olur.
    fn finish<T, R>(
        &self,
        rec: Recorder,
        result: Result<T>,
        wrap: impl FnOnce(T, DiagReport) -> R,
    ) -> Result<R> {
        let report = match &result {
            Ok(_) => rec.finish(Ok(())),
            Err(err) => rec.finish(Err(err)),
        };
        if let Err(write_err) = crate::diag::save_last_run(&self.config.last_run_path(), &report) {
            // Tanı yazılamadıysa asıl hatayı gizleme; yalnızca logla.
            tracing::warn!(error = %write_err.chain_text(), "tanı raporu yazılamadı");
        }
        result.map(|value| wrap(value, report))
    }
}

/// Onay defterinde kaydı olmayan eklenti için ortak hata.
///
/// `disable`/`enable`/`forget` hiç onaylanmamış bir eklentide sessizce
/// başarılı olmamalı: kullanıcı bir şey yaptığını sanır (K9).
fn missing_consent(found: bool, name: &str) -> Result<()> {
    if found {
        Ok(())
    } else {
        Err(Error::new(
            Stage::PluginLoad,
            ErrorKind::PluginNotApproved {
                plugin: name.to_owned(),
                detail: "onay defterinde kaydı yok — önce `headshell plugin approve`".to_owned(),
            },
        ))
    }
}

/// Bir eklentinin beyan ettiği araçları motorla kurar (D-055). Hiç araç
/// istemiyorsa ağa çıkılmaz ve boş liste döner.
fn install_artifacts(
    store: &ArtifactStore,
    manifest: &PluginManifest,
) -> Result<Vec<(String, InstallOutcome)>> {
    let mut outcomes = Vec::new();
    if !manifest.requires.is_empty() {
        let source = crate::plugin::artifact::default_artifact_source()?;
        for requirement in &manifest.requires {
            let outcome = store.install(source.as_ref(), requirement)?;
            outcomes.push((requirement.name.clone(), outcome));
        }
    }
    Ok(outcomes)
}

/// Yola göre zip mi dizin mi olduğuna karar verir.
fn open_archive(path: &Path) -> Result<Box<dyn import::ExportArchive>> {
    let meta = std::fs::metadata(path)
        .map_err(|source| crate::error::io_err(Stage::ImportRead, path, source))?;
    if meta.is_dir() {
        Ok(Box::new(import::DirArchive::open(path)?))
    } else if meta.is_file() {
        Ok(Box::new(import::ZipArchive::open(path)?))
    } else {
        Err(Error::new(
            Stage::ImportRead,
            ErrorKind::InvalidInput {
                detail: format!("{} ne dosya ne dizin", path.display()),
            },
        ))
    }
}

/// Kimlik zincirinin üstveri kaynağını nereden alacağı.
///
/// Ağa çıkmak **açık bir tercih**: `headshell` ağ olmadan da eksiksiz çalışan bir
/// araçtır ve bir export'u içe aktarmak kimseyi sessizce MusicBrainz'e
/// bağlamamalı. Seçim çekirdekte duruyor ki GUI ve mobil aynı iki seçeneği
/// aynı adlarla sunsun (Altın Kural).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupMode {
    /// Ağ yok: zincir ISRC'den öteye gitmez, gerisi `LocalKey`'e düşer.
    #[default]
    Offline,
    /// MusicBrainz'e sorar — zincirin 2. ve 3. halkası çalışır.
    ///
    /// **Yavaştır ve bu kaçınılmaz:** MusicBrainz anonim istemciye saniyede
    /// bir istek veriyor, yani binlerce parçalık bir export saatler sürer.
    /// Tek parçalık `resolve` için uygun, toplu içe aktarma için değil.
    Online,
}

/// Verilen kipin üstveri kaynağı.
///
/// Dönüş tipi somut değil `Arc<dyn MetadataLookup>` (D-006): çağıranlar
/// — CLI, GUI, mobil — kaynağı değiştirdiğimizde imza görmeden geçsin.
///
/// # Errors
/// [`LookupMode::Online`] istendi ama bu derlemede HTTP istemcisi yok
/// (`http-client` feature'ı kapalı). Sessizce çevrimdışına düşmüyoruz:
/// kullanıcı ağ istediğini söyledi ve neden alamadığını bilmeli (K9).
pub fn lookup_for(mode: LookupMode) -> Result<Arc<dyn MetadataLookup>> {
    match mode {
        LookupMode::Offline => Ok(default_lookup()),
        LookupMode::Online => crate::identity::musicbrainz::default_musicbrainz_lookup(),
    }
}

/// Varsayılan üstveri kaynağı: ağ yok.
#[must_use]
pub fn default_lookup() -> Arc<dyn MetadataLookup> {
    Arc::new(OfflineLookup)
}
