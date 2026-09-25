//! Eklenti sınırı: gömülü QuickJS'te koşan sağlayıcılar (K5, D-069).
//!
//! Bir eklenti `<data_dir>/plugins/<ad>/` dizinidir; içinde `plugin.json`
//! (bkz. [`manifest`]) ve bir JS betiği vardır. Betik çekirdeğin içindeki
//! motorda koşar — kullanıcının makinesinde **hiçbir şey kurulu olması
//! gerekmez**. api 1'de eklenti Python'la yazılmış bir alt süreçti ve "kime
//! göndersem bir sorun çıktı"nın sebebi buydu: Windows'ta Python yok,
//! Debian'da `venv` ayrı paket, sürümler tutmuyor.
//!
//! Eklentiler ana depoda durmaz: `headshell/plugins` deposunda yaşarlar ve
//! uygulama onları o deponun indeksinden kurar ve günceller ([`catalog`],
//! D-071). Elle konmuş bir dizin de eklentidir; katalog ona dokunmaz.
//!
//! ## Yaşam döngüsü
//!
//! 1. **Keşif** ([`discover`]) — motoru açmadan: manifest okunur, izin onayı
//!    ([`consent`]) sorulur, protokol sürümü ve eserlerin durumu ölçülür.
//!    `headshell plugin list` bu kadarını kullanır.
//! 2. **Başlatma** — **ilk çağrıda**, tembel: eklentinin iş parçacığı açılır,
//!    betik değerlendirilir ve beyan edilen yeteneklerin fonksiyonları
//!    dışa aktarılmış mı diye bakılır ([`script`]).
//! 3. **Çağrı** — her çağrının süresi var; takılan eklenti çekirdeği
//!    takmaz.
//! 4. **Düşme** — zaman aşımı ya da düşen iş parçacığı motoru bırakır ve bir
//!    sonraki çağrı yeniden başlatır. [`MAX_STARTS`] denemeden sonra
//!    vazgeçilir; sonsuz yeniden başlatma bir çökme döngüsünü gizler.
//!
//! ## Sınır neyi tutar
//!
//! Kimlik alanı (eklenti başka sağlayıcının kimliğini uyduramaz), sır alanı
//! (yalnızca kendi ad alanı, D-042), zaman, bellek, **ağ** (her istek ve her
//! yönlendirme `permissions.net`'e göre denetlenir) ve **dosya sistemi**
//! (eklentinin dosyaya erişimi yok). Tutmadığı tek kapı motorun kurduğu
//! araçlar: `host.tools.run` ile çalışan yt-dlp gibi bir eser kullanıcının
//! yetkisiyle çalışır ve bu `headshell plugin list`'te yazar ([`host`]).

pub mod artifact;
pub mod catalog;
pub mod consent;
#[cfg(feature = "plugin-engine")]
mod host;
pub mod manifest;
pub mod protocol;
#[cfg(feature = "plugin-engine")]
mod script;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::ids::{ProviderId, ProviderTrackId};
use crate::net::HttpClient;
use crate::provider::{
    AudioSource, Capabilities, Provider, ProviderFuture, ProviderHealth, ProviderInfo,
    ProviderTrack,
};
use crate::secrets::{Secrets, plugin_namespace};

use artifact::ArtifactStore;
use consent::{ConsentStatus, ConsentStore};
use manifest::{MANIFEST_FILE, Permissions, PluginManifest, Requirement};
use protocol::{HealthResult, PLUGIN_API, SourceResult, WireTrack, export};

#[cfg(feature = "plugin-engine")]
use script::ScriptWorker;
#[cfg(not(feature = "plugin-engine"))]
use unavailable::ScriptWorker;

/// İzin beyanı zorlanıyor mu (D-040 → D-069).
///
/// api 1'de `false`'tu: eklenti ayrı bir süreçti ve kullanıcının bütün
/// yetkisiyle çalışıyordu. api 2'de eklenti dışarıya yalnızca motorun
/// kapılarından çıkabiliyor ve motor her kapıda beyana bakıyor. **Tek
/// istisna** motorun kurduğu araçlar (yt-dlp): onlar ayrı süreçtir ve
/// hapsedilmez — çıktılar bunu ayrıca söyler.
pub const PERMISSIONS_ENFORCED: bool = true;

/// Bir eklentinin kaç kez başlatılacağı. Aşılırsa vazgeçilir ve sebebi
/// söylenir — sonsuz yeniden başlatma çökme döngüsünü sessiz kılar.
pub const MAX_STARTS: u32 = 3;

/// Betiği değerlendirmeye tanınan süre. Kısa: yükleme ağa çıkamaz
/// (motor bunu reddeder), yalnızca kendini kurar.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// Bir çağrıya tanınan süre. Ağ gerektiren bir arama ya da yt-dlp'nin imza
/// çözümü bu kadar sürebilir.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// Keşifte görülen bir eklenti. Çalıştırılamayanlar da burada — sessizce
/// atlanan eklenti, kullanıcının kurduğunu sandığı eklentidir (K9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEntry {
    /// Dizin adı = kimlik.
    pub name: String,
    pub dir: PathBuf,
    /// Manifest okunabildiyse ondan gelen alanlar.
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub api: Option<u32>,
    pub permissions: Permissions,
    /// Onay durumu (manifest okunabildiyse).
    pub consent: Option<ConsentStatus>,
    /// Motordan istenen eserlerin durumu (D-055, D-069). **Ağa çıkılmadan**
    /// ölçülür: bu platform için yayın var mı, diskte mi, karması tutuyor mu.
    #[serde(default)]
    pub requires: Vec<artifact::RequirementStatus>,
    /// Yüklenemiyorsa sebebi — tek satır, kopyalanabilir.
    pub problem: Option<String>,
}

impl PluginEntry {
    /// Bu eklenti başlatılabilir mi.
    ///
    /// Eksik bir eser yüklemeyi **engeller**: eseri olmayan bir eklentiyi
    /// başlatmak, onu ilk aramada anlaşılmaz bir hatayla düşürmek olurdu.
    #[must_use]
    pub fn is_loadable(&self) -> bool {
        self.problem.is_none()
            && self.missing_requirements().is_empty()
            && self
                .consent
                .as_ref()
                .is_some_and(consent::ConsentStatus::is_approved)
    }

    /// Hazır olmayan eserler. Boşsa motor tarafında eksik yok.
    #[must_use]
    pub fn missing_requirements(&self) -> Vec<&artifact::RequirementStatus> {
        self.requires
            .iter()
            .filter(|status| !status.state.is_ready())
            .collect()
    }

    /// Kullanıcıya gösterilecek tek satırlık durum.
    #[must_use]
    pub fn status_text(&self) -> String {
        if let Some(problem) = &self.problem {
            return problem.clone();
        }
        let missing = self.missing_requirements();
        if !missing.is_empty() {
            let detail = missing
                .iter()
                .map(|status| {
                    format!(
                        "{} {}: {}",
                        status.name,
                        status.version,
                        status.state.describe()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            // Platform desteği yoksa kurulum komutu önermek yanlış tavsiye.
            let unsupported = missing.iter().all(|status| {
                matches!(status.state, artifact::RequirementState::Unsupported { .. })
            });
            return if unsupported {
                format!("motorun kurması gereken eser bu platformda yok ({detail})")
            } else {
                format!(
                    "motorun kurması gereken eser eksik ({detail}) — `headshell plugin install {}`",
                    self.name
                )
            };
        }
        match &self.consent {
            Some(status) => status.describe(),
            None => "durum bilinmiyor".to_owned(),
        }
    }
}

/// Keşfin özeti (K9: kaç geldi, kaçı ne oldu).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSummary {
    /// Dizinde görülen eklenti sayısı.
    pub discovered: usize,
    /// Yüklenmeye hazır (onaylı, sürümü uygun, eserleri kurulu).
    pub ready: usize,
    /// Onay bekliyor ya da yeni izin istiyor.
    pub awaiting_approval: usize,
    /// Kullanıcı kapatmış.
    pub disabled: usize,
    /// Protokol sürümü uyuşmuyor.
    pub incompatible: usize,
    /// Manifesti okunamadı/geçersiz.
    pub broken: usize,
    /// Motorun kurması gereken bir eseri eksik (D-055). `ready`'den ayrı:
    /// kullanıcının yapacağı şey farklı — onay değil kurulum.
    #[serde(default)]
    pub needs_install: usize,
}

impl PluginSummary {
    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
        recorder.set("plugins.discovered", n(self.discovered));
        recorder.set("plugins.ready", n(self.ready));
        recorder.set("plugins.awaiting_approval", n(self.awaiting_approval));
        recorder.set("plugins.disabled", n(self.disabled));
        recorder.set("plugins.incompatible", n(self.incompatible));
        recorder.set("plugins.broken", n(self.broken));
        recorder.set("plugins.needs_install", n(self.needs_install));
    }
}

/// Eklenti dizinini tarar. **Hiçbir eklentiyi başlatmaz.**
///
/// Bozuk bir eklenti taramayı durdurmaz: sebebi [`PluginEntry::problem`]'e
/// yazılır ve gerisi taranmaya devam eder.
///
/// # Errors
/// Eklenti dizini okunamazsa (var ama izin yok gibi) ya da onay defteri
/// bozuksa. Dizin **yoksa** hata değil: boş liste.
pub fn discover(config: &Config) -> Result<(Vec<PluginEntry>, PluginSummary)> {
    let dir = config.plugins_dir();
    let consents = ConsentStore::load(&config.plugin_consent_path())?;
    let store = ArtifactStore::new(config);

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), PluginSummary::default()));
        }
        Err(err) => return Err(io_err(Stage::PluginLoad, &dir, err)),
    };

    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| io_err(Stage::PluginLoad, &dir, err))?;
        let path = entry.path();
        if !path.is_dir() || !path.join(MANIFEST_FILE).exists() {
            continue;
        }
        found.push(describe_plugin(&path, &consents, &store));
    }
    // Dizin sırası dosya sistemine göre değişir; çıktı kararlı olmalı.
    found.sort_by(|a, b| a.name.cmp(&b.name));

    let summary = summarize(&found);
    Ok((found, summary))
}

fn summarize(entries: &[PluginEntry]) -> PluginSummary {
    let mut summary = PluginSummary {
        discovered: entries.len(),
        ..PluginSummary::default()
    };
    for entry in entries {
        if entry.problem.is_some() {
            if entry.api.is_some_and(|api| api != PLUGIN_API) {
                summary.incompatible += 1;
            } else {
                summary.broken += 1;
            }
            continue;
        }
        if !entry.missing_requirements().is_empty() {
            summary.needs_install += 1;
            continue;
        }
        match &entry.consent {
            Some(ConsentStatus::Approved) => summary.ready += 1,
            Some(ConsentStatus::Disabled) => summary.disabled += 1,
            Some(ConsentStatus::NotAsked | ConsentStatus::NeedsApproval { .. }) => {
                summary.awaiting_approval += 1;
            }
            None => summary.broken += 1,
        }
    }
    summary
}

fn describe_plugin(dir: &Path, consents: &ConsentStore, store: &ArtifactStore) -> PluginEntry {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let manifest = match PluginManifest::load(dir) {
        Ok(manifest) => manifest,
        Err(err) => {
            let api = match err.kind() {
                ErrorKind::PluginIncompatible { plugin_api, .. } => Some(*plugin_api),
                _ => None,
            };
            let problem = match err.kind() {
                ErrorKind::PluginIncompatible { plugin_api, .. } => format!(
                    "protokol sürümü uyuşmuyor: eklenti api {plugin_api}, çekirdek api \
                     {PLUGIN_API}{}",
                    if *plugin_api == 1 {
                        " — api 1 eski Python/alt süreç eklentisiydi; eklentinin api 2 \
                         (QuickJS) sürümünü kurun"
                    } else {
                        ""
                    }
                ),
                _ => err.chain_text().replace('\n', " "),
            };
            return PluginEntry {
                name,
                dir: dir.to_path_buf(),
                display_name: None,
                version: None,
                api,
                permissions: Permissions::default(),
                consent: None,
                requires: Vec::new(),
                problem: Some(problem),
            };
        }
    };

    // Eser durumu diskten okunuyor; okunamazsa bu da bir `problem` — sessizce
    // "eksik yok" demek, eksik bir eseri hazır göstermek olurdu.
    let (requires, problem) = match store.statuses(&manifest.requires) {
        Ok(requires) => (requires, None),
        Err(err) => (Vec::new(), Some(err.chain_text().replace('\n', " "))),
    };

    PluginEntry {
        name: name.clone(),
        dir: dir.to_path_buf(),
        display_name: Some(manifest.display_name.clone()),
        version: manifest.version.clone(),
        api: Some(manifest.api),
        consent: Some(consents.status(&name, &manifest.permissions)),
        permissions: manifest.permissions,
        requires,
        problem,
    }
}

/// Onaylı eklentileri sağlayıcı olarak kurar. **Motor açılmaz** — her
/// sağlayıcı ilk çağrısında kendi motorunu açar.
///
/// # Errors
/// Keşif başarısız olursa ya da sır dosyası bozuksa.
pub fn load(config: &Config) -> Result<(Vec<Arc<dyn Provider>>, PluginSummary)> {
    let (entries, summary) = discover(config)?;
    let secrets = Secrets::load(&config.secrets_path())?;

    let mut providers: Vec<Arc<dyn Provider>> = Vec::new();
    for entry in &entries {
        if !entry.is_loadable() {
            tracing::debug!(plugin = %entry.name, durum = %entry.status_text(), "eklenti yüklenmedi");
            continue;
        }
        // Keşif manifesti bir kez okudu; kurulum için yeniden okuyoruz
        // çünkü `PluginEntry` uniffi'ye taşınabilir dar bir görünüm (K7),
        // manifestin tamamı değil.
        let manifest = match PluginManifest::load(&entry.dir) {
            Ok(manifest) => manifest,
            Err(err) => {
                tracing::warn!(
                    plugin = %entry.name,
                    error = %err.chain_text().replace('\n', " "),
                    "eklenti manifesti keşiften sonra okunamadı"
                );
                continue;
            }
        };
        providers.push(Arc::new(PluginProvider::from_manifest(
            config, &manifest, &entry.dir, &secrets,
        )?));
    }
    Ok((providers, summary))
}

/// Bir eklentinin motoru başlatmak için gereken her şey.
///
/// Değer olarak taşınıyor, çünkü motor kendi iş parçacığında kuruluyor ve
/// her yeniden başlatma aynı tariften yapılıyor.
///
/// Motor kapalı bir derlemede alanların çoğunu okuyan kimse yok — yedek
/// yalnızca adı kullanıp "motor yok" diyor. Tarif yine de kuruluyor, çünkü
/// sağlayıcının keşif, onay ve yetenek yüzü motordan bağımsız ve aynı kalmalı.
#[derive(Clone)]
#[cfg_attr(not(feature = "plugin-engine"), allow(dead_code))]
pub(crate) struct ScriptSpec {
    pub(crate) plugin: String,
    /// Betiğin mutlak yolu.
    pub(crate) main: PathBuf,
    /// Yığın izlerinde görünecek ad: manifestteki `main`.
    pub(crate) module_name: String,
    pub(crate) capabilities: Capabilities,
    pub(crate) permissions: Permissions,
    pub(crate) secrets: BTreeMap<String, String>,
    pub(crate) state_dir: PathBuf,
    /// Eklentinin HTTP istemcisi; yoksa **neden** olmadığı (K9).
    pub(crate) http: std::result::Result<Arc<dyn HttpClient>, String>,
    pub(crate) store: ArtifactStore,
    pub(crate) requires: Vec<Requirement>,
}

/// Gömülü motorda yaşayan bir sağlayıcı.
pub struct PluginProvider {
    id: ProviderId,
    display_name: String,
    capabilities: Capabilities,
    permissions: Permissions,
    spec: ScriptSpec,
    start_timeout: Duration,
    call_timeout: Duration,
    state: std::sync::Mutex<SessionState>,
}

#[derive(Default)]
struct SessionState {
    worker: Option<ScriptWorker>,
    starts: u32,
    /// Bir daha denemeye değmeyen bir sebep (sözleşme ihlali gibi).
    give_up: Option<String>,
}

impl std::fmt::Debug for PluginProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginProvider")
            .field("id", &self.id)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl PluginProvider {
    /// Manifestten kurar, bu derlemenin eklenti HTTP istemcisiyle. Motor
    /// açılmaz.
    ///
    /// # Errors
    /// Eklentinin durum dizini oluşturulamazsa.
    pub fn from_manifest(
        config: &Config,
        manifest: &PluginManifest,
        dir: &Path,
        secrets: &Secrets,
    ) -> Result<Self> {
        let http = crate::net::plugin_http_client().map_err(|err| {
            format!(
                "bu derlemede eklentiler ağa çıkamaz: {}",
                err.chain_text().replace('\n', " ")
            )
        });
        Self::with_http(config, manifest, dir, secrets, http)
    }

    /// Manifestten kurar; HTTP istemcisini çağıran verir. Motor açılmaz.
    ///
    /// Testler ve kendi HTTP yığınını taşıyan kabuklar için. Verilen istemci
    /// **yönlendirme izlememeli**: izleyen bir istemci izin denetiminin
    /// etrafından dolanmanın yolunu açar ([`host`]).
    ///
    /// # Errors
    /// Eklentinin durum dizini oluşturulamazsa.
    pub fn with_http(
        config: &Config,
        manifest: &PluginManifest,
        dir: &Path,
        secrets: &Secrets,
        http: std::result::Result<Arc<dyn HttpClient>, String>,
    ) -> Result<Self> {
        let state_dir = config.plugin_state_dir(&manifest.name);
        std::fs::create_dir_all(&state_dir)
            .map_err(|err| io_err(Stage::PluginLoad, &state_dir, err))?;

        let (capabilities, unknown) = protocol::parse_capabilities(&manifest.capabilities);
        if !unknown.is_empty() {
            tracing::warn!(
                plugin = %manifest.name,
                unknown = %unknown.join(", "),
                "manifest tanınmayan yetenek adı içeriyor, yok sayıldı"
            );
        }
        let unmapped = capabilities.contains(Capabilities::BROWSE)
            || capabilities.contains(Capabilities::CONTROL);
        if unmapped {
            tracing::warn!(
                plugin = %manifest.name,
                "`browse`/`control` api 2'de bir fonksiyona karşılık gelmiyor; beyan yalnızca listede görünür"
            );
        }

        let spec = ScriptSpec {
            plugin: manifest.name.clone(),
            main: manifest.main_path(dir),
            module_name: manifest.main.trim_start_matches("./").to_owned(),
            capabilities,
            permissions: manifest.permissions.normalized(),
            secrets: secrets.namespace(&plugin_namespace(&manifest.name)),
            state_dir,
            http,
            store: ArtifactStore::new(config),
            requires: manifest.requires.clone(),
        };

        Ok(Self {
            id: ProviderId::new(manifest.name.clone()),
            display_name: manifest.display_name.clone(),
            capabilities,
            permissions: manifest.permissions.normalized(),
            spec,
            start_timeout: START_TIMEOUT,
            call_timeout: CALL_TIMEOUT,
            state: std::sync::Mutex::new(SessionState::default()),
        })
    }

    /// Süreleri değiştirir. Yalnızca sınama için: zaman aşımını sınayan bir
    /// test 20 saniye beklememeli.
    #[must_use]
    pub fn with_timeouts(mut self, start: Duration, call: Duration) -> Self {
        self.start_timeout = start;
        self.call_timeout = call;
        self
    }

    /// Motoru hazırlar (gerekirse başlatır) ve bir fonksiyonu çağırır.
    ///
    /// Zaman aşımı ve düşen iş parçacığı motoru bırakır: bir sonraki çağrı
    /// yeniden başlatır. Eklentinin fırlattığı hata motoru bırakmaz —
    /// reddedilen bir istek bozulmuş bir motor değildir (D-023'ün eklenti
    /// hâli).
    fn call(
        &self,
        function: &'static str,
        args: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut state = self.state.lock().map_err(|_| {
            Error::new(
                Stage::ProviderCall,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: "eklenti durumu kilidi bozuldu (önceki çağrı panikledi)".to_owned(),
                },
            )
        })?;

        if let Some(reason) = &state.give_up {
            return Err(Error::new(
                Stage::PluginStart,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: reason.clone(),
                },
            ));
        }

        if state.worker.is_none() {
            self.start(&mut state)?;
        }
        let Some(worker) = state.worker.as_mut() else {
            return Err(Error::new(
                Stage::PluginStart,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: "eklenti başlatılamadı".to_owned(),
                },
            ));
        };

        let outcome = worker.call(function, args, self.call_timeout);
        if let Err(err) = &outcome
            && matches!(
                err.kind(),
                ErrorKind::PluginCrashed { .. } | ErrorKind::PluginTimeout { .. }
            )
        {
            // Süresi dolan bir çağrının yarım bıraktığı durumla devam
            // edilmiyor: motor bırakılır, sıradaki çağrı temiz başlar.
            state.worker = None;
        }
        outcome
    }

    fn start(&self, state: &mut SessionState) -> Result<()> {
        if state.starts >= MAX_STARTS {
            let reason = format!("{MAX_STARTS} kez başlatıldı ve her seferinde düştü, vazgeçildi");
            state.give_up = Some(reason.clone());
            return Err(Error::new(
                Stage::PluginStart,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: reason,
                },
            ));
        }
        state.starts += 1;

        match ScriptWorker::start(self.spec.clone(), self.start_timeout) {
            Ok(worker) => {
                state.worker = Some(worker);
                Ok(())
            }
            Err(err) => {
                // Sözleşme ihlali ve eksik motor tekrarla düzelmez.
                if matches!(
                    err.kind(),
                    ErrorKind::PluginContract { .. } | ErrorKind::Unsupported { .. }
                ) {
                    state.give_up = Some(err.chain_text().replace('\n', " "));
                }
                Err(err)
            }
        }
    }

    /// Motoru kapatır. Bir sonraki çağrı yeniden başlatır.
    pub fn shutdown(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                if let Some(mut worker) = state.worker.take() {
                    worker.shutdown();
                }
            }
            Err(_) => tracing::warn!(plugin = %self.id, "kapatma sırasında kilit bozuktu"),
        }
    }

    fn unsupported(&self, what: &str) -> Error {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::Unsupported {
                provider: self.id.as_str().to_owned(),
                what: what.to_owned(),
                capabilities: self.capabilities.describe(),
            },
        )
    }

    fn contract(&self, method: &str, detail: String) -> Error {
        Error::new(
            Stage::ProviderCall,
            ErrorKind::PluginContract {
                plugin: self.id.as_str().to_owned(),
                method: method.to_owned(),
                detail,
            },
        )
    }

    fn health_now(&self) -> Result<HealthResult> {
        let value = self.call(export::HEALTH, Vec::new())?;
        serde_json::from_value(value).map_err(|err| {
            self.contract(
                export::HEALTH,
                format!("`{{ reachable, detail?, track_count? }}` bekleniyordu: {err}"),
            )
        })
    }

    fn search_now(&self, query: &str, limit: usize) -> Result<Vec<ProviderTrack>> {
        if !self.capabilities.contains(Capabilities::SEARCH) {
            return Err(self.unsupported("arama"));
        }
        let value = self.call(
            export::SEARCH,
            vec![serde_json::json!(query), serde_json::json!(limit)],
        )?;
        let wires: Vec<WireTrack> = serde_json::from_value(value).map_err(|err| {
            self.contract(
                export::SEARCH,
                format!("`{{ id, artist, title, … }}` dizisi bekleniyordu: {err}"),
            )
        })?;

        let mut tracks = Vec::with_capacity(wires.len());
        let mut dropped_isrc = 0usize;
        for wire in wires {
            let (track, dropped) = wire.into_provider_track(&self.id);
            if dropped {
                dropped_isrc += 1;
            }
            tracks.push(track);
        }
        if dropped_isrc > 0 {
            // Sayıp raporluyoruz, yutmuyoruz (K9).
            tracing::warn!(
                plugin = %self.id,
                dropped = dropped_isrc,
                "eklenti biçimsiz ISRC gönderdi, o alanlar düşürüldü"
            );
        }
        Ok(tracks)
    }

    fn resolve_now(&self, id: &ProviderTrackId) -> Result<Option<AudioSource>> {
        if !self.capabilities.contains(Capabilities::STREAM) {
            return Err(self.unsupported("kaynak çözme"));
        }
        if id.provider != self.id {
            return Err(Error::new(
                Stage::PlaybackResolve,
                ErrorKind::InvalidInput {
                    detail: format!("{} kimliği {} eklentisine sorulamaz", id.provider, self.id),
                },
            ));
        }
        let value = self.call(export::RESOLVE_SOURCE, vec![serde_json::json!(id.id)])?;
        let source: SourceResult = serde_json::from_value(value).map_err(|err| {
            self.contract(
                export::RESOLVE_SOURCE,
                format!(
                    "`{{ kind: \"http_stream\", url, headers }}` ya da `null` bekleniyordu: {err}"
                ),
            )
        })?;
        if let Some(source) = &source
            && let Err(reason) = protocol::check_source(source, &self.permissions)
        {
            return Err(self.contract(export::RESOLVE_SOURCE, reason));
        }
        Ok(source)
    }
}

impl Provider for PluginProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            capabilities: self.capabilities,
        }
    }

    fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth> {
        Box::pin(std::future::ready(Ok(match self.health_now() {
            Ok(health) => ProviderHealth {
                id: self.id.clone(),
                reachable: health.reachable,
                track_count: health.track_count,
                detail: health.detail,
            },
            // Ulaşılamamak bir sağlık **cevabıdır** (uzak sağlayıcıyla aynı
            // kural): `provider test` sebebi göstermeli.
            Err(err) => ProviderHealth {
                id: self.id.clone(),
                reachable: false,
                track_count: None,
                detail: Some(err.chain_text().replace('\n', " ")),
            },
        })))
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> ProviderFuture<'a, Vec<ProviderTrack>> {
        Box::pin(std::future::ready(self.search_now(query, limit)))
    }

    fn resolve_source<'a>(
        &'a self,
        id: &'a ProviderTrackId,
    ) -> ProviderFuture<'a, Option<AudioSource>> {
        Box::pin(std::future::ready(self.resolve_now(id)))
    }
}

impl Drop for PluginProvider {
    fn drop(&mut self) {
        // Sağlayıcı düşerse arkasında iş parçacığı ve sır dosyası kalmaz.
        self.shutdown();
    }
}

/// Bir future'ı çağıran iş parçacığında bitirir.
///
/// Çekirdek bir çalışma zamanı kurmuyor (konvansiyon: çalışma zamanını
/// çağıran seçer), ama motorun iki yeri eşzamanlı: eklentinin iş
/// parçacığındaki `host.http` ve kurulum komutu. `HttpClient::send` ise
/// `async`. Aradaki boşluk burada kapanıyor.
///
/// Bekleme **meşgul değil**: future hazır değilse iş parçacığı uyutulur ve
/// uyandırıcı onu kaldırır. `ureq` istemcisi zaten ilk yoklamada hazır
/// dönüyor; kendi eşzamansız istemcisini veren bir kabuk (mobil) da işlemci
/// yakmadan beklenir.
pub(crate) fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    use std::pin::pin;
    use std::task::{Context, Poll, Wake, Waker};

    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

/// Motorun olmadığı derlemenin yedeği: aynı yüzey, her başlatma bir hata.
///
/// Eklentiler bu derlemede de keşfedilir, listelenir ve onaylanır —
/// kullanıcı neyin kurulu olduğunu görmeli. Yalnızca **çalıştırılamazlar**
/// ve ilk çağrı bunu söyler (K9).
#[cfg(not(feature = "plugin-engine"))]
mod unavailable {
    use std::time::Duration;

    use super::ScriptSpec;
    use crate::diag::Stage;
    use crate::error::{Error, ErrorKind, Result};

    #[derive(Debug)]
    pub(crate) struct ScriptWorker;

    fn missing(plugin: &str) -> Error {
        Error::new(
            Stage::PluginStart,
            ErrorKind::Unsupported {
                provider: plugin.to_owned(),
                what: "eklenti çalıştırma (`plugin-engine` feature'ı kapalı bir derleme)"
                    .to_owned(),
                capabilities: "NONE".to_owned(),
            },
        )
    }

    impl ScriptWorker {
        pub(crate) fn start(spec: ScriptSpec, _timeout: Duration) -> Result<Self> {
            Err(missing(&spec.plugin))
        }

        pub(crate) fn call(
            &mut self,
            _function: &'static str,
            _args: Vec<serde_json::Value>,
            _timeout: Duration,
        ) -> Result<serde_json::Value> {
            Err(missing("?"))
        }

        pub(crate) fn shutdown(&mut self) {}
    }
}

#[cfg(test)]
mod tests;
