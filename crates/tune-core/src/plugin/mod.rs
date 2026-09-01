//! Eklenti sınırı: alt süreç + JSON-RPC sağlayıcılar (K5, Faz 2 §2.1).
//!
//! Bir eklenti `<data_dir>/plugins/<ad>/` dizinidir; içinde `plugin.json`
//! (bkz. [`manifest`]) ve çalıştırılabilir bir şey vardır. Dil serbest —
//! protokolün tamamı satır bazlı JSON ([`protocol`]).
//!
//! ## Yaşam döngüsü
//!
//! 1. **Keşif** ([`discover`]) — süreç açmadan: manifest okunur, izin onayı
//!    ([`consent`]) sorulur, protokol sürümü karşılaştırılır. `tune plugin
//!    list` bu kadarını kullanır ve hiçbir eklentiyi başlatmaz.
//! 2. **Başlatma** — **ilk çağrıda**, tembel. `tune stats` çalışırken
//!    yanında altı süreç açılmasın diye; ve el sıkışmadaki yetenekler
//!    manifestte de yazdığı için başlatmadan yönlendirme yapılabiliyor.
//! 3. **El sıkışma** — sürüm denetlenir. Uymuyorsa eklenti yüklenmez,
//!    çekirdek çökmez, iki sayı da kullanıcıya söylenir.
//! 4. **Çağrı** — her metodun zaman aşımı var; asılı kalan eklenti
//!    çekirdeği asmaz.
//! 5. **Çökme** — süreç ölürse çağrı hata döner ve bir sonraki çağrıda
//!    yeniden başlatılır. [`MAX_STARTS`] denemeden sonra vazgeçilir; sonsuz
//!    yeniden başlatma bir çökme döngüsünü gizler.
//!
//! ## Sınır neyi tutar, neyi tutmaz
//!
//! Tutar: kimlik alanı (eklenti başka bir sağlayıcının kimliğini uyduramaz,
//! bkz. [`protocol::WireTrack`]), sır alanı (yalnızca kendi ad alanı, D-042),
//! zaman (zaman aşımı), yaşam (çökme izolasyonu).
//!
//! Tutmaz: dosya sistemi ve ağ. Eklenti kullanıcının bütün yetkisiyle
//! çalışır; izin beyanı bir sözleşmedir, güvenlik duvarı değil (D-040) — ve
//! bu, `tune plugin list` çıktısında da böyle yazar.

pub mod client;
pub mod consent;
pub mod manifest;
pub mod protocol;
pub mod transport;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::ids::{ProviderId, ProviderTrackId};
use crate::provider::{
    AudioSource, Capabilities, Provider, ProviderFuture, ProviderHealth, ProviderInfo,
    ProviderTrack,
};
use crate::secrets::{Secrets, plugin_namespace};

use client::{CALL_TIMEOUT, HANDSHAKE_TIMEOUT, PluginClient};
use consent::{ConsentStatus, ConsentStore};
use manifest::{MANIFEST_FILE, Permissions, PluginManifest};
use protocol::{
    HandshakeParams, HandshakeResult, HealthResult, HostInfo, PLUGIN_API, ResolveSourceParams,
    ResolveSourceResult, SearchParams, SearchResult, method,
};
use transport::{ProcessFactory, TransportFactory};

/// İzin beyanı zorlanıyor mu (D-040).
///
/// `false`, ve bu sabit **dışa açık** çünkü her çıktıda görünmesi gerekiyor:
/// eklenti kullanıcının bütün yetkisiyle çalışır, beyan bir sözleşmedir.
/// Landlock/bwrap geldiğinde bu değer platforma göre hesaplanacak — o gün
/// `api` artmaz, yalnızca burası değişir.
pub const PERMISSIONS_ENFORCED: bool = false;

/// Bir eklentinin kaç kez başlatılacağı. Aşılırsa vazgeçilir ve sebebi
/// söylenir — sonsuz yeniden başlatma çökme döngüsünü sessiz kılar.
pub const MAX_STARTS: u32 = 3;

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
    /// Yüklenemiyorsa sebebi — tek satır, kopyalanabilir.
    pub problem: Option<String>,
}

impl PluginEntry {
    /// Bu eklenti başlatılabilir mi.
    #[must_use]
    pub fn is_loadable(&self) -> bool {
        self.problem.is_none()
            && self
                .consent
                .as_ref()
                .is_some_and(consent::ConsentStatus::is_approved)
    }

    /// Kullanıcıya gösterilecek tek satırlık durum.
    #[must_use]
    pub fn status_text(&self) -> String {
        if let Some(problem) = &self.problem {
            return problem.clone();
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
    /// Yüklenmeye hazır (onaylı, sürümü uygun).
    pub ready: usize,
    /// Onay bekliyor ya da yeni izin istiyor.
    pub awaiting_approval: usize,
    /// Kullanıcı kapatmış.
    pub disabled: usize,
    /// Protokol sürümü uyuşmuyor.
    pub incompatible: usize,
    /// Manifesti okunamadı/geçersiz.
    pub broken: usize,
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
    }
}

/// Eklenti dizinini tarar. **Hiçbir süreç başlatmaz.**
///
/// Bozuk bir eklenti taramayı durdurmaz: sebebi [`PluginEntry::problem`]'e
/// yazılır ve gerisi taranmaya devam eder. Bir eklentinin bozuk olması
/// ötekileri görünmez yapmamalı.
///
/// # Errors
/// Eklenti dizini okunamazsa (var ama izin yok gibi) ya da onay defteri
/// bozuksa. Dizin **yoksa** hata değil: boş liste.
pub fn discover(config: &Config) -> Result<(Vec<PluginEntry>, PluginSummary)> {
    let dir = config.plugins_dir();
    let consents = ConsentStore::load(&config.plugin_consent_path())?;

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
        found.push(describe_plugin(&path, &consents));
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
            if entry.api.is_some() {
                summary.incompatible += 1;
            } else {
                summary.broken += 1;
            }
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

fn describe_plugin(dir: &Path, consents: &ConsentStore) -> PluginEntry {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let manifest = match PluginManifest::load(dir) {
        Ok(manifest) => manifest,
        Err(err) => {
            return PluginEntry {
                name,
                dir: dir.to_path_buf(),
                display_name: None,
                version: None,
                api: None,
                permissions: Permissions::default(),
                consent: None,
                problem: Some(err.chain_text().replace('\n', " ")),
            };
        }
    };

    // Sürüm uyuşmazlığı **keşifte** yakalanıyor: süreç açmaya gerek yok ve
    // kullanıcı sebebini `plugin list`'te görüyor.
    let problem = (manifest.api != PLUGIN_API).then(|| {
        format!(
            "protokol sürümü uyuşmuyor: eklenti api {}, çekirdek api {PLUGIN_API}",
            manifest.api
        )
    });

    PluginEntry {
        name: name.clone(),
        dir: dir.to_path_buf(),
        display_name: Some(manifest.display_name.clone()),
        version: manifest.version.clone(),
        api: Some(manifest.api),
        consent: Some(consents.status(&name, &manifest.permissions)),
        permissions: manifest.permissions,
        problem,
    }
}

/// Onaylı eklentileri sağlayıcı olarak kurar. **Süreç başlatmaz** — her
/// sağlayıcı ilk çağrısında kendi sürecini açar.
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

/// Alt süreçte yaşayan bir sağlayıcı.
pub struct PluginProvider {
    id: ProviderId,
    display_name: String,
    /// El sıkışmadan sonra güncellenir; başlangıçta manifestteki beyan.
    capabilities: AtomicU32,
    manifest_path: PathBuf,
    factory: Arc<dyn TransportFactory>,
    handshake: HandshakeParams,
    state: std::sync::Mutex<SessionState>,
}

#[derive(Default)]
struct SessionState {
    client: Option<PluginClient>,
    starts: u32,
    /// Bir daha denemeye değmeyen bir sebep (sürüm uyuşmazlığı gibi).
    give_up: Option<String>,
}

impl std::fmt::Debug for PluginProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginProvider")
            .field("id", &self.id)
            .field("factory", &self.factory)
            .finish()
    }
}

impl PluginProvider {
    /// Manifestten kurar. Süreç açılmaz.
    ///
    /// # Errors
    /// Eklentinin durum dizini oluşturulamazsa.
    pub fn from_manifest(
        config: &Config,
        manifest: &PluginManifest,
        dir: &Path,
        secrets: &Secrets,
    ) -> Result<Self> {
        let (program, args) = manifest.resolve_exec(dir);
        let factory = Arc::new(ProcessFactory::new(
            &manifest.name,
            program,
            args,
            dir.to_path_buf(),
        ));

        let state_dir = config.plugin_state_dir(&manifest.name);
        std::fs::create_dir_all(&state_dir)
            .map_err(|err| io_err(Stage::PluginLoad, &state_dir, err))?;

        let handshake = HandshakeParams {
            api: PLUGIN_API,
            host: HostInfo::current(),
            data_dir: protocol::path_to_wire(&state_dir),
            secrets: secrets.namespace(&plugin_namespace(&manifest.name)),
            permissions: manifest.permissions.normalized(),
        };

        Ok(Self::new(
            manifest,
            dir.join(MANIFEST_FILE),
            factory,
            handshake,
        ))
    }

    /// Taşımayı çağıran verir — testler süreç açmadan sınayabilsin diye.
    #[must_use]
    pub fn new(
        manifest: &PluginManifest,
        manifest_path: PathBuf,
        factory: Arc<dyn TransportFactory>,
        handshake: HandshakeParams,
    ) -> Self {
        let (declared, unknown) = protocol::parse_capabilities(&manifest.capabilities);
        if !unknown.is_empty() {
            tracing::warn!(
                plugin = %manifest.name,
                unknown = %unknown.join(", "),
                "manifest tanınmayan yetenek adı içeriyor, yok sayıldı"
            );
        }
        Self {
            id: ProviderId::new(manifest.name.clone()),
            display_name: manifest.display_name.clone(),
            capabilities: AtomicU32::new(declared.bits()),
            manifest_path,
            factory,
            handshake,
            state: std::sync::Mutex::new(SessionState::default()),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::from_bits(self.capabilities.load(Ordering::Relaxed))
    }

    /// İstemciyi hazırlar (gerekirse başlatır) ve `run`'ı çalıştırır.
    ///
    /// Çökme ve zaman aşımı istemciyi düşürür: bir sonraki çağrı yeniden
    /// başlatır. Öteki hatalar (eklenti "hayır" dedi) süreci düşürmez —
    /// reddedilen bir istek ölmüş bir süreç değildir (D-023'ün eklenti hâli).
    fn with_client<T>(
        &self,
        stage: Stage,
        run: impl FnOnce(&mut PluginClient) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.state.lock().map_err(|_| {
            Error::new(
                stage,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: "eklenti durumu kilidi bozuldu (önceki çağrı panikledi)".to_owned(),
                },
            )
        })?;

        if let Some(reason) = &state.give_up {
            return Err(Error::new(
                stage,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: reason.clone(),
                },
            ));
        }

        if state.client.is_none() {
            self.start(&mut state)?;
        }
        let Some(client) = state.client.as_mut() else {
            return Err(Error::new(
                stage,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: "eklenti başlatılamadı".to_owned(),
                },
            ));
        };

        let outcome = run(client);
        if let Err(err) = &outcome
            && matches!(
                err.kind(),
                ErrorKind::PluginCrashed { .. } | ErrorKind::PluginTimeout { .. }
            )
        {
            // Asılı kalan süreci de düşürüyoruz: cevap vermeyen bir eklentiyle
            // sıradaki çağrıda id'ler karışırdı.
            if let Some(client) = state.client.as_mut() {
                client.shutdown();
            }
            state.client = None;
        }
        outcome
    }

    fn start(&self, state: &mut SessionState) -> Result<()> {
        if state.starts >= MAX_STARTS {
            let reason = format!("{MAX_STARTS} kez başlatıldı ve her seferinde düştü, vazgeçildi");
            state.give_up = Some(reason.clone());
            return Err(Error::new(
                Stage::PluginHandshake,
                ErrorKind::PluginCrashed {
                    plugin: self.id.as_str().to_owned(),
                    detail: reason,
                },
            ));
        }
        state.starts += 1;

        let transport = self.factory.open()?;
        let mut client = PluginClient::new(self.id.as_str(), transport);
        match self.handshake(&mut client) {
            Ok(()) => {
                state.client = Some(client);
                Ok(())
            }
            Err(err) => {
                client.shutdown();
                // Sürüm uyuşmazlığı yeniden denemekle düzelmez.
                if matches!(err.kind(), ErrorKind::PluginIncompatible { .. }) {
                    state.give_up = Some(err.chain_text().replace('\n', " "));
                }
                Err(err)
            }
        }
    }

    fn handshake(&self, client: &mut PluginClient) -> Result<()> {
        let params = serde_json::to_value(&self.handshake).map_err(|source| {
            Error::new(
                Stage::PluginHandshake,
                ErrorKind::Json {
                    entry: format!("{} el sıkışma isteği", self.id),
                    source,
                },
            )
        })?;
        let result: HandshakeResult = client.call(
            Stage::PluginHandshake,
            method::HANDSHAKE,
            params,
            HANDSHAKE_TIMEOUT,
        )?;

        if result.api != PLUGIN_API {
            return Err(Error::new(
                Stage::PluginHandshake,
                ErrorKind::PluginIncompatible {
                    plugin: self.id.as_str().to_owned(),
                    plugin_api: result.api,
                    host_api: PLUGIN_API,
                },
            ));
        }
        if result.name != self.id.as_str() {
            return Err(Error::new(
                Stage::PluginHandshake,
                ErrorKind::PluginManifest {
                    path: self.manifest_path.clone(),
                    detail: format!(
                        "el sıkışmada kendini `{}` diye tanıttı, manifest `{}` diyor",
                        result.name, self.id
                    ),
                },
            ));
        }

        let (live, unknown) = protocol::parse_capabilities(&result.capabilities);
        if !unknown.is_empty() {
            tracing::warn!(
                plugin = %self.id,
                unknown = %unknown.join(", "),
                "eklenti tanımadığımız bir yetenek bildirdi, yok sayıldı"
            );
        }
        let declared = self.capabilities();
        if live != declared {
            // Çelişki hata değil ama sessiz de değil: yönlendirme manifeste
            // bakıyor, çağrı el sıkışmaya (K9).
            tracing::warn!(
                plugin = %self.id,
                manifest = %declared.describe(),
                handshake = %live.describe(),
                "manifest ve el sıkışma yetenekleri farklı, el sıkışma geçerli"
            );
        }
        self.capabilities.store(live.bits(), Ordering::Relaxed);
        Ok(())
    }

    /// Süreci kapatır. Bir sonraki çağrı yeniden başlatır.
    pub fn shutdown(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                if let Some(client) = state.client.as_mut() {
                    client.shutdown();
                }
                state.client = None;
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
                capabilities: self.capabilities().describe(),
            },
        )
    }
}

impl Provider for PluginProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            capabilities: self.capabilities(),
        }
    }

    fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth> {
        Box::pin(std::future::ready({
            let result = self.with_client(Stage::ProviderCall, |client| {
                client.call::<HealthResult>(
                    Stage::ProviderCall,
                    method::HEALTH,
                    serde_json::json!({}),
                    CALL_TIMEOUT,
                )
            });
            match result {
                Ok(health) => Ok(ProviderHealth {
                    id: self.id.clone(),
                    reachable: health.reachable,
                    track_count: health.track_count,
                    detail: health.detail,
                }),
                // Ulaşılamamak bir sağlık **cevabıdır** (uzak sağlayıcıyla
                // aynı kural): `provider test` sebebi göstermeli.
                Err(err) => Ok(ProviderHealth {
                    id: self.id.clone(),
                    reachable: false,
                    track_count: None,
                    detail: Some(err.chain_text().replace('\n', " ")),
                }),
            }
        }))
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> ProviderFuture<'a, Vec<ProviderTrack>> {
        Box::pin(std::future::ready((|| {
            if !self.capabilities().contains(Capabilities::SEARCH) {
                return Err(self.unsupported("arama"));
            }
            let params = serde_json::to_value(SearchParams {
                query: query.to_owned(),
                limit,
            })
            .map_err(|source| {
                Error::new(
                    Stage::ProviderCall,
                    ErrorKind::Json {
                        entry: format!("{} arama isteği", self.id),
                        source,
                    },
                )
            })?;

            let result: SearchResult = self.with_client(Stage::ProviderCall, |client| {
                client.call(Stage::ProviderCall, method::SEARCH, params, CALL_TIMEOUT)
            })?;

            let mut tracks = Vec::with_capacity(result.tracks.len());
            let mut dropped_isrc = 0usize;
            for wire in result.tracks {
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
        })()))
    }

    fn resolve_source<'a>(
        &'a self,
        id: &'a ProviderTrackId,
    ) -> ProviderFuture<'a, Option<AudioSource>> {
        Box::pin(std::future::ready((|| {
            if !self.capabilities().contains(Capabilities::STREAM) {
                return Err(self.unsupported("kaynak çözme"));
            }
            if id.provider != self.id {
                return Err(Error::new(
                    Stage::PlaybackResolve,
                    ErrorKind::InvalidInput {
                        detail: format!(
                            "{} kimliği {} eklentisine sorulamaz",
                            id.provider, self.id
                        ),
                    },
                ));
            }
            let params = serde_json::to_value(ResolveSourceParams { id: id.id.clone() }).map_err(
                |source| {
                    Error::new(
                        Stage::ProviderCall,
                        ErrorKind::Json {
                            entry: format!("{} kaynak isteği", self.id),
                            source,
                        },
                    )
                },
            )?;

            let result: ResolveSourceResult = self.with_client(Stage::ProviderCall, |client| {
                client.call(
                    Stage::ProviderCall,
                    method::RESOLVE_SOURCE,
                    params,
                    CALL_TIMEOUT,
                )
            })?;
            Ok(result.source)
        })()))
    }
}

impl Drop for PluginProvider {
    fn drop(&mut self) {
        // Sağlayıcı düşerse arkasında süreç kalmaz.
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use transport::{PluginTransport, Received, ScriptedTransport};

    /// Her açılışta sırayla verilen betikleri oynatan fabrika: çökme ve
    /// yeniden başlatma böyle sınanıyor, süreç açmadan.
    #[derive(Debug)]
    struct FakeFactory {
        scripts: Mutex<Vec<Vec<String>>>,
        opens: Mutex<usize>,
    }

    impl FakeFactory {
        fn new(scripts: Vec<Vec<&str>>) -> Arc<Self> {
            Arc::new(Self {
                scripts: Mutex::new(
                    scripts
                        .into_iter()
                        .map(|lines| lines.into_iter().map(str::to_owned).collect())
                        .collect(),
                ),
                opens: Mutex::new(0),
            })
        }

        fn opens(&self) -> usize {
            *self.opens.lock().unwrap()
        }
    }

    impl TransportFactory for FakeFactory {
        fn open(&self) -> Result<Box<dyn PluginTransport>> {
            *self.opens.lock().unwrap() += 1;
            let mut scripts = self.scripts.lock().unwrap();
            let lines = if scripts.is_empty() {
                Vec::new()
            } else {
                scripts.remove(0)
            };
            Ok(Box::new(ScriptedTransport::new(
                lines.into_iter().map(Received::Line).collect(),
            )))
        }
    }

    fn manifest() -> PluginManifest {
        PluginManifest {
            name: "demo".to_owned(),
            display_name: "Demo".to_owned(),
            version: Some("0.1.0".to_owned()),
            api: PLUGIN_API,
            exec: vec!["demo".to_owned()],
            capabilities: vec!["search".to_owned(), "stream".to_owned()],
            permissions: Permissions::default(),
            description: None,
        }
    }

    fn provider(factory: Arc<dyn TransportFactory>) -> PluginProvider {
        PluginProvider::new(
            &manifest(),
            PathBuf::from("/tmp/demo/plugin.json"),
            factory,
            HandshakeParams {
                api: PLUGIN_API,
                host: HostInfo::current(),
                data_dir: "/tmp/demo/state".to_owned(),
                secrets: std::collections::BTreeMap::new(),
                permissions: Permissions::default(),
            },
        )
    }

    const HANDSHAKE_OK: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"api":1,"name":"demo","display_name":"Demo","capabilities":["search","stream"]}}"#;

    #[tokio::test]
    async fn a_search_goes_through_the_handshake_and_comes_back_typed() {
        let factory = FakeFactory::new(vec![vec![
            HANDSHAKE_OK,
            r#"{"jsonrpc":"2.0","id":2,"result":{"tracks":[{"id":"42","artist":"A","title":"B","duration_ms":1000}]}}"#,
        ]]);
        let provider = provider(factory.clone());

        let tracks = provider.search("b", 10).await.unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id.provider, ProviderId::new("demo"));
        assert_eq!(tracks[0].id.id, "42");
        assert_eq!(tracks[0].track.title, "B");
        assert_eq!(factory.opens(), 1, "süreç bir kez açılmalı");
    }

    #[tokio::test]
    async fn the_process_starts_lazily_on_the_first_call_not_at_construction() {
        let factory = FakeFactory::new(vec![vec![HANDSHAKE_OK]]);
        let provider = provider(factory.clone());
        assert_eq!(factory.opens(), 0, "kurulum süreç açmamalı");
        // `info` de açmamalı: yetenekler manifestten biliniyor.
        assert!(provider.info().capabilities.contains(Capabilities::SEARCH));
        assert_eq!(factory.opens(), 0);
    }

    #[tokio::test]
    async fn a_version_mismatch_refuses_the_plugin_without_crashing_the_core() {
        let factory = FakeFactory::new(vec![vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"api":99,"name":"demo","display_name":"Demo","capabilities":[]}}"#,
        ]]);
        let provider = provider(factory.clone());

        let err = provider.search("x", 1).await.unwrap_err();
        match err.kind() {
            ErrorKind::PluginIncompatible {
                plugin_api,
                host_api,
                ..
            } => {
                assert_eq!(*plugin_api, 99);
                assert_eq!(*host_api, PLUGIN_API);
            }
            other => panic!("beklenmeyen hata: {other:?}"),
        }
        assert_eq!(err.stage(), Stage::PluginHandshake);

        // İkinci çağrı yeniden denemez: sürüm uyuşmazlığı tekrarla düzelmez.
        let second = provider.search("x", 1).await.unwrap_err();
        assert!(matches!(second.kind(), ErrorKind::PluginCrashed { .. }));
        assert_eq!(factory.opens(), 1, "vazgeçilen eklenti yeniden açılmamalı");
    }

    #[tokio::test]
    async fn a_plugin_that_lies_about_its_name_is_rejected() {
        let factory = FakeFactory::new(vec![vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"api":1,"name":"baska","display_name":"X","capabilities":[]}}"#,
        ]]);
        let provider = provider(factory);
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            err.chain_text().contains("el sıkışmada kendini"),
            "{}",
            err.chain_text()
        );
    }

    #[tokio::test]
    async fn a_crash_is_isolated_and_the_next_call_restarts_the_process() {
        let factory = FakeFactory::new(vec![
            // İlk süreç: el sıkışır, sonra ölür.
            vec![HANDSHAKE_OK],
            // İkinci süreç: el sıkışır ve cevabı verir.
            vec![
                HANDSHAKE_OK,
                r#"{"jsonrpc":"2.0","id":2,"result":{"tracks":[]}}"#,
            ],
        ]);
        let provider = provider(factory.clone());

        let err = provider.search("x", 1).await.unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::PluginCrashed { .. }));

        let tracks = provider.search("x", 1).await.unwrap();
        assert!(tracks.is_empty());
        assert_eq!(factory.opens(), 2, "çöken eklenti yeniden başlatılmalı");
    }

    #[tokio::test]
    async fn restarting_gives_up_after_max_starts() {
        // Her açılış hemen ölüyor: sonsuza kadar denenmemeli.
        let factory = FakeFactory::new(vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()]);
        let provider = provider(factory.clone());

        for _ in 0..MAX_STARTS {
            assert!(provider.search("x", 1).await.is_err());
        }
        let err = provider.search("x", 1).await.unwrap_err();
        assert!(
            err.chain_text().contains("vazgeçildi"),
            "{}",
            err.chain_text()
        );
        assert_eq!(factory.opens(), MAX_STARTS as usize);
    }

    #[tokio::test]
    async fn a_rejected_call_does_not_kill_the_process() {
        let factory = FakeFactory::new(vec![vec![
            HANDSHAKE_OK,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"kota doldu"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"result":{"tracks":[]}}"#,
        ]]);
        let provider = provider(factory.clone());

        let err = provider.search("x", 1).await.unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::PluginRpc { .. }));

        // Reddedilen istek ağ hatası değildir (D-023) — eklenti hâli:
        // reddedilen istek çökme değildir, süreç ayakta kalmalı.
        provider.search("x", 1).await.unwrap();
        assert_eq!(factory.opens(), 1, "reddetme süreci düşürmemeli");
    }

    #[tokio::test]
    async fn health_reports_a_dead_plugin_as_unreachable_not_as_an_error() {
        let factory = FakeFactory::new(vec![Vec::new()]);
        let provider = provider(factory);
        let health = provider.health().await.unwrap();
        assert!(!health.reachable);
        assert!(health.detail.is_some_and(|detail| detail.contains("ADIM:")));
    }

    #[tokio::test]
    async fn a_capability_the_plugin_lacks_is_refused_before_the_process_starts() {
        let mut manifest = manifest();
        manifest.capabilities = vec!["search".to_owned()];
        let factory = FakeFactory::new(vec![vec![HANDSHAKE_OK]]);
        let provider = PluginProvider::new(
            &manifest,
            PathBuf::from("/tmp/demo/plugin.json"),
            factory.clone(),
            HandshakeParams {
                api: PLUGIN_API,
                host: HostInfo::current(),
                data_dir: "/tmp".to_owned(),
                secrets: std::collections::BTreeMap::new(),
                permissions: Permissions::default(),
            },
        );

        let id = ProviderTrackId::new(ProviderId::new("demo"), "1");
        let err = provider.resolve_source(&id).await.unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Unsupported { .. }));
        assert_eq!(factory.opens(), 0, "yeteneği olmayan çağrı süreç açmamalı");
    }

    #[tokio::test]
    async fn a_track_id_from_another_provider_is_refused() {
        let factory = FakeFactory::new(vec![vec![HANDSHAKE_OK]]);
        let provider = provider(factory.clone());
        let id = ProviderTrackId::new(ProviderId::new("baska"), "1");
        let err = provider.resolve_source(&id).await.unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::InvalidInput { .. }));
        assert_eq!(factory.opens(), 0);
    }

    // --- keşif ---

    fn temp_config(name: &str) -> Config {
        let dir = std::env::temp_dir().join(format!(
            "tune-plugin-discover-{}-{}-{name}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Config::with_data_dir(dir)
    }

    fn write_plugin(config: &Config, name: &str, manifest_json: &str) {
        let dir = config.plugins_dir().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), manifest_json).unwrap();
    }

    #[test]
    fn a_missing_plugins_dir_is_an_empty_list_not_an_error() {
        let config = temp_config("bos");
        let (entries, summary) = discover(&config).unwrap();
        assert!(entries.is_empty());
        assert_eq!(summary, PluginSummary::default());
    }

    #[test]
    fn discovery_reports_each_plugins_reason_without_stopping() {
        let config = temp_config("karisik");
        write_plugin(
            &config,
            "iyi",
            r#"{"name":"iyi","display_name":"İyi","api":1,"exec":["x"],
                "permissions":{"net":["a.example"]}}"#,
        );
        write_plugin(
            &config,
            "eski",
            r#"{"name":"eski","display_name":"Eski","api":99,"exec":["x"]}"#,
        );
        write_plugin(&config, "bozuk", "{ bu json değil");

        let (entries, summary) = discover(&config).unwrap();
        assert_eq!(entries.len(), 3, "bozuk eklenti ötekileri gizlememeli");
        assert_eq!(summary.discovered, 3);
        assert_eq!(summary.incompatible, 1);
        assert_eq!(summary.broken, 1);
        assert_eq!(summary.awaiting_approval, 1, "onay bekleyen: iyi");
        assert_eq!(summary.ready, 0);

        let by_name = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap()
                .clone()
        };
        assert!(by_name("eski").problem.unwrap().contains("api 99"));
        assert!(by_name("bozuk").problem.is_some());
        assert!(
            !by_name("iyi").is_loadable(),
            "onaysız eklenti yüklenmemeli"
        );
    }

    #[test]
    fn an_approved_plugin_becomes_loadable_and_load_starts_no_process() {
        let config = temp_config("onayli");
        write_plugin(
            &config,
            "iyi",
            r#"{"name":"iyi","display_name":"İyi","api":1,"exec":["/bin/false"],
                "capabilities":["search"],"permissions":{"net":["a.example"]}}"#,
        );

        let mut store = ConsentStore::default();
        store.approve(
            "iyi",
            &Permissions {
                net: vec!["a.example".to_owned()],
                fs: Vec::new(),
            },
            jiff::Timestamp::now(),
        );
        store.save(&config.plugin_consent_path()).unwrap();

        let (entries, summary) = discover(&config).unwrap();
        assert_eq!(summary.ready, 1);
        assert!(entries[0].is_loadable());

        let (providers, summary) = load(&config).unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(summary.ready, 1);
        assert_eq!(providers[0].info().id, ProviderId::new("iyi"));
        assert!(
            providers[0]
                .info()
                .capabilities
                .contains(Capabilities::SEARCH)
        );
        // Eklentinin durum dizini kurulmuş olmalı.
        assert!(config.plugin_state_dir("iyi").is_dir());
    }

    #[test]
    fn a_plugin_that_grew_its_permissions_is_not_loaded_until_reapproved() {
        let config = temp_config("buyuyen");
        write_plugin(
            &config,
            "iyi",
            r#"{"name":"iyi","display_name":"İyi","api":1,"exec":["x"],
                "permissions":{"net":["a.example","yeni.example"]}}"#,
        );
        let mut store = ConsentStore::default();
        store.approve(
            "iyi",
            &Permissions {
                net: vec!["a.example".to_owned()],
                fs: Vec::new(),
            },
            jiff::Timestamp::now(),
        );
        store.save(&config.plugin_consent_path()).unwrap();

        let (entries, summary) = discover(&config).unwrap();
        assert_eq!(summary.awaiting_approval, 1);
        assert!(!entries[0].is_loadable());
        assert!(
            entries[0].status_text().contains("yeni.example"),
            "{}",
            entries[0].status_text()
        );

        let (providers, _) = load(&config).unwrap();
        assert!(providers.is_empty());
    }

    #[test]
    fn a_plugin_only_sees_its_own_secrets() {
        let config = temp_config("sirlar");
        write_plugin(
            &config,
            "iyi",
            r#"{"name":"iyi","display_name":"İyi","api":1,"exec":["x"],"capabilities":["search"]}"#,
        );
        let mut store = ConsentStore::default();
        store.approve("iyi", &Permissions::default(), jiff::Timestamp::now());
        store.save(&config.plugin_consent_path()).unwrap();

        let mut secrets = Secrets::default();
        secrets.set("plugin:iyi", "client_id", "benim");
        secrets.set("plugin:baska", "client_id", "onun");
        secrets.save(&config.secrets_path()).unwrap();

        let manifest = PluginManifest::load(&config.plugins_dir().join("iyi")).unwrap();
        let provider = PluginProvider::from_manifest(
            &config,
            &manifest,
            &config.plugins_dir().join("iyi"),
            &Secrets::load(&config.secrets_path()).unwrap(),
        )
        .unwrap();

        assert_eq!(provider.handshake.secrets.len(), 1);
        assert_eq!(
            provider.handshake.secrets.get("client_id"),
            Some(&"benim".to_owned())
        );
    }
}
