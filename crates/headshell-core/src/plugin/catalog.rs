//! Eklenti kataloğu: ayrı bir depodan okunan `index.json` (D-071).
//!
//! Eklentiler ana depoda durmuyor. [`headshell/plugins`] deposunda dizin
//! olarak yaşıyorlar ve o deponun kökündeki `index.json` onlardan üretiliyor
//! ([`build_index`]). Uygulama listeyi bu dosyadan okur, eklentiyi oradan
//! kurar ve günceller.
//!
//! [`headshell/plugins`]: https://github.com/headshell/plugins
//!
//! ## İndeks (şema 1)
//!
//! ```json
//! {
//!   "schema": 1,
//!   "url_template": "https://…/refs/tags/{name}-{version}/{name}/{path}",
//!   "plugins": [
//!     {
//!       "manifest": { "name": "soundcloud", "version": "0.2.0", … },
//!       "files": [
//!         { "path": "plugin.json", "url": "https://…", "sha256": "…" },
//!         { "path": "main.js", "url": "https://…", "sha256": "…" }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! Girdi eklentinin manifestini **olduğu gibi** taşıyor: katalogda
//! gösterilen izinler ile kurulan eklentinin izinleri aynı kaynaktan geliyor,
//! ve inen `plugin.json` bununla karşılaştırılıyor. Dosya adresi serbest:
//! bugün hepsi `headshell/plugins`'in sürüm etiketlerinde, ama başka bir
//! depodaki bir eklenti de aynı biçimle listelenebilir (Obsidian'ın modeli).
//! `url_template` yalnızca üreticinin notu; istemci okumaz.
//!
//! ## Güven
//!
//! İndeks HTTPS'ten gelir ve her dosyanın sha256'sını taşır:
//!
//! 1. Her dosya karmasıyla doğrulanır; biri tutmazsa diske hiçbir şey
//!    yazılmaz.
//! 2. İnen `plugin.json` indeksin gösterdiği manifestle **aynı** olmalı.
//! 3. Kurulan eklenti **onay bekler** (D-040): katalogdan gelmek onay değil.
//!
//! Adresleri sürüm etiketine sabitlemek üreticinin işi ve şablonda
//! `{version}` bu yüzden zorunlu: GitHub'ın ham içerik önbelleği beş dakika
//! tutuyor ve `main`'e sabitli bir adres, yeni bir sürüm yayımlanırken yeni
//! indeksi eski dosyayla eşleştirirdi — "karma tutmuyor" diyen, korkutucu ve
//! geçici bir hata.
//!
//! ## Ağ
//!
//! Katalog yalnızca **açık bir komutla** okunur: `plugin catalog`,
//! `plugin install` (eklenti diskte yoksa) ve `plugin update`. Açılışta ya da
//! arka planda istek yok — "bir export'u içe aktarmak kimseyi sessizce ağa
//! bağlamaz" ilkesinin aynısı.
//!
//! ## Kimin dosyasına dokunulur
//!
//! Katalogdan kurulan her eklentinin dizininde bir köken kaydı durur
//! ([`ORIGIN_FILE`]): hangi katalog, hangi sürüm, hangi dosyalar ve
//! karmaları. Güncelleme **yalnızca** bu kaydı olan ve dosyaları kayıtla aynı
//! olan eklentiye dokunur. Elle konmuş (kayıt yok) ya da elle değiştirilmiş
//! (karma tutmuyor) bir eklentinin üstüne yazılmaz: bir geliştiricinin
//! çalışma kopyası bir güncellemeyle silinmemeli.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::net::{HttpClient, HttpRequest};

use super::artifact::{hash_file, sha256_hex, short_hash};
use super::manifest::{
    MANIFEST_FILE, Permissions, PluginManifest, Requirement, url_host, validate_catalog_name,
    validate_local_name,
};
use super::protocol::PLUGIN_API;

/// Varsayılan katalog: `headshell/plugins` deposunun `main` dalındaki indeks.
///
/// İndeks dalda, dosyalar sürüm etiketlerinde: indeks her zaman güncel
/// listeyi verir, gösterdiği dosyalar ise değişmez.
pub const DEFAULT_INDEX_URL: &str =
    "https://raw.githubusercontent.com/headshell/plugins/refs/heads/main/index.json";

/// Katalog adresini değiştirmek için ortam değişkeni: bir çatal, bir ayna,
/// ya da sınama için yerel bir sunucu.
pub const INDEX_ENV: &str = "HEADSHELL_PLUGIN_INDEX";

/// Katalog deposunun kökündeki indeks dosyasının adı.
pub const INDEX_FILE: &str = "index.json";

/// Bu çekirdeğin okuduğu indeks biçimi.
///
/// Kural `api`'ninkiyle aynı: **eklemek artırmaz**, kaldırmak ya da anlamını
/// değiştirmek artırır. Bilinmeyen bir şema okunmaz ve bunun sebebi söylenir.
pub const INDEX_SCHEMA: u32 = 1;

/// Katalogdan kurulan eklentinin dizinindeki köken kaydı.
pub const ORIGIN_FILE: &str = "origin.json";

/// İndeksin kabul edilen en büyük boyutu. Emniyet kemeri: yanlış bir adres
/// belleği doldurmasın.
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;

/// Bir eklenti dosyasının kabul edilen en büyük boyutu. Bugünün en büyük
/// eklentisi ~12 KB.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Eklenti dizininde motorun kullandığı adlar: katalogdaki bir dosya bunlara
/// yazılamaz. `state/` motorun eklenti adına tuttuğu depo
/// ([`Config::plugin_state_dir`]), `origin.json` köken kaydı.
const RESERVED: &[&str] = &[ORIGIN_FILE, "state"];

/// Katalog adresini ortamdan çözer: `HEADSHELL_PLUGIN_INDEX` ya da
/// varsayılan. Dışarıdan [`Config::plugin_index_url`] ile çağrılır; ortam bir
/// closure olarak geçtiği için dışa açılmaz (K7).
#[must_use]
pub(crate) fn resolve_index_url(env: &dyn Fn(&str) -> Option<String>) -> String {
    env(INDEX_ENV)
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_INDEX_URL.to_owned())
}

/// Katalogdaki bir eklenti dosyası.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFile {
    /// Eklenti dizinine göre yol, `/` ayırıcılı (`main.js`).
    pub path: String,
    /// İndirileceği adres.
    pub url: String,
    /// Beklenen sha256, küçük harf onaltılık. Tutmazsa dosya yazılmaz.
    pub sha256: String,
}

/// Katalogdaki bir eklentinin bu makinedeki durumu. **Ağa çıkmadan**
/// ölçülür; "güncelleme var" katalogdaki sürümle karşılaştırmadır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum InstallState {
    /// Bu makinede yok.
    NotInstalled,
    /// Katalogdan kuruldu, sürüm katalogdakiyle aynı, dosyalar kurulduğu gibi.
    Current { version: String },
    /// Katalogdan kuruldu; katalogda başka bir sürüm var.
    UpdateAvailable {
        installed: String,
        available: String,
    },
    /// Dizin var ama köken kaydı yok: elle konmuş. Güncelleme dokunmaz.
    Manual,
    /// Katalogdan kuruldu ama dosyaları kurulduğu gibi değil. Güncelleme
    /// dokunmaz — elle yapılan değişiklik silinmesin.
    Modified { version: String, files: Vec<String> },
    /// Köken kaydı okunamadı. Güncelleme dokunmaz.
    Unreadable { detail: String },
}

impl InstallState {
    /// Diskte bu adla bir eklenti var mı.
    #[must_use]
    pub const fn is_installed(&self) -> bool {
        !matches!(self, Self::NotInstalled)
    }

    /// Kullanıcıya gösterilecek tek satır.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NotInstalled => "kurulu değil".to_owned(),
            Self::Current { version } => format!("kurulu {version} · güncel"),
            Self::UpdateAvailable {
                installed,
                available,
            } => format!("güncelleme var: {installed} → {available}"),
            Self::Manual => "elle kurulmuş — katalog ona dokunmaz (köken kaydı yok)".to_owned(),
            Self::Modified { version, files } => format!(
                "kurulu {version} · yerelde değiştirilmiş ({}) — güncelleme üstüne yazmaz",
                files.join(", ")
            ),
            Self::Unreadable { detail } => format!("köken kaydı okunamadı: {detail}"),
        }
    }
}

/// Katalogdaki bir eklenti — kullanıcıya gösterilen hâli.
///
/// Kurulamayan girdiler de burada: sessizce düşen bir girdi, kullanıcının
/// katalogda aradığı ama bulamadığı eklentidir (K9). Sebebi `problem`'de.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPlugin {
    pub name: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub api: Option<u32>,
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Eklentinin beyan ettiği ağ izinleri — kurulunca onaya sunulacak olan.
    #[serde(default)]
    pub permissions: Permissions,
    /// Motorun kuracağı araçlar (D-055). Ağ izninden ayrı gösterilir.
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub files: Vec<CatalogFile>,
    /// Kurulamıyorsa sebebi, tek satır.
    pub problem: Option<String>,
    pub installed: InstallState,
}

/// Bir katalog okumasının özeti (K9: kaç geldi, kaçı ne durumda).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSummary {
    /// İndeksteki girdi sayısı.
    pub listed: usize,
    /// Bu sürümün kurabileceği girdiler.
    pub installable: usize,
    /// Bu makinede kurulu olanlar (elle konmuşlar dahil).
    pub installed: usize,
    /// Güncellemesi olanlar.
    pub updates: usize,
    /// Kurulamayan girdiler (bozuk, uyumsuz sürüm).
    pub problems: usize,
}

impl CatalogSummary {
    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
        recorder.set("catalog.listed", n(self.listed));
        recorder.set("catalog.installable", n(self.installable));
        recorder.set("catalog.installed", n(self.installed));
        recorder.set("catalog.updates", n(self.updates));
        recorder.set("catalog.problems", n(self.problems));
    }
}

/// Kataloğun bu makineye karşı okunmuş hâli.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSurvey {
    pub plugins: Vec<CatalogPlugin>,
    /// **Bu katalogdan** kurulmuş ama artık listede olmayan eklentiler. Bir
    /// eklenti katalogdan çekildiyse kullanıcı bunu bilmeli.
    pub delisted: Vec<String>,
    pub summary: CatalogSummary,
}

/// Katalogdan kurulan bir eklentinin köken kaydı ([`ORIGIN_FILE`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRecord {
    /// Okunduğu katalog.
    pub index: String,
    pub version: String,
    pub installed_at: jiff::Timestamp,
    /// Yol → sha256. Güncelleme bunlara bakıp yerel değişikliği yakalar.
    pub files: BTreeMap<String, String>,
}

/// Bu komutta katalogdan ne indirildiği.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFetch {
    pub index: String,
    pub version: String,
    pub files: Vec<CatalogFile>,
}

/// Bir aracın (motorun kurduğu eser) güncellemede değişmesi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolChange {
    pub name: String,
    /// Önceki sürüm; `None`: yeni eklendi.
    pub from: Option<String>,
    /// Yeni sürüm; `None`: artık istenmiyor.
    pub to: Option<String>,
    /// Bu platformun ikilisinin adresi ya da karması değişti mi — sürüm aynı
    /// kalsa bile.
    pub binary_changed: bool,
}

impl ToolChange {
    /// Kullanıcıya gösterilecek tek satır.
    #[must_use]
    pub fn describe(&self) -> String {
        match (&self.from, &self.to) {
            (None, Some(to)) => format!("{} {to} eklendi", self.name),
            (Some(from), None) => format!("{} {from} artık istenmiyor", self.name),
            (Some(from), Some(to)) if from != to => format!("{} {from} → {to}", self.name),
            (Some(version), Some(_)) => format!(
                "{} {version}: sürüm aynı, bu platformun ikilisi değişti",
                self.name
            ),
            (None, None) => self.name.clone(),
        }
    }
}

/// Bir güncelleme denemesinin sonucu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// Yeni sürüm yerine kondu.
    Updated {
        from: String,
        to: String,
        /// Yeni sürümün **fazladan** istediği ağ izinleri. Boş değilse
        /// eklenti yeniden onay bekler (D-040).
        permissions_added: Permissions,
        /// Motorun kurduğu araçlardaki değişiklikler. Onay istemez (D-071,
        /// kullanıcının kararı) ama **söylenir**: hapsedilmeyen bir ikili
        /// sessizce değişmemeli.
        tools_changed: Vec<ToolChange>,
    },
    /// Kurulu sürüm katalogdakiyle aynı.
    Current { version: String },
    /// Dokunulmadı; sebebi yazıyor.
    Skipped { reason: String },
    /// Denendi ve olmadı (ağ, karma, disk). Eklenti eski hâlinde.
    Failed { error: String },
}

impl UpdateOutcome {
    /// Kullanıcıya gösterilecek tek satır.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Updated { from, to, .. } => format!("güncellendi: {from} → {to}"),
            Self::Current { version } => format!("güncel ({version})"),
            Self::Skipped { reason } => format!("atlandı — {reason}"),
            Self::Failed { error } => format!("GÜNCELLENEMEDİ — {error}"),
        }
    }
}

/// Bir güncelleme turunun özeti (K9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSummary {
    pub checked: usize,
    pub updated: usize,
    pub current: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl UpdateSummary {
    /// Sonuçlardan sayar.
    #[must_use]
    pub fn of<'a>(outcomes: impl IntoIterator<Item = &'a UpdateOutcome>) -> Self {
        let mut summary = Self::default();
        for outcome in outcomes {
            summary.checked += 1;
            match outcome {
                UpdateOutcome::Updated { .. } => summary.updated += 1,
                UpdateOutcome::Current { .. } => summary.current += 1,
                UpdateOutcome::Skipped { .. } => summary.skipped += 1,
                UpdateOutcome::Failed { .. } => summary.failed += 1,
            }
        }
        summary
    }

    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
        recorder.set("update.checked", n(self.checked));
        recorder.set("update.updated", n(self.updated));
        recorder.set("update.current", n(self.current));
        recorder.set("update.skipped", n(self.skipped));
        recorder.set("update.failed", n(self.failed));
    }
}

/// Kaldırılan bir eklenti.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub path: PathBuf,
    /// Dizin bir sembolik bağlantıydıysa hedefi. Yalnızca **bağlantı**
    /// kaldırıldı; hedefteki dosyalara dokunulmadı — bir geliştiricinin
    /// çalışma kopyası olabilir.
    pub link_target: Option<PathBuf>,
}

/// İndekse giren bir eklenti.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedPlugin {
    pub name: String,
    pub version: String,
    pub files: Vec<CatalogFile>,
}

/// Üretilmiş bir indeks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltIndex {
    /// Dosyaya yazılacak metin (sonunda satır sonu).
    pub json: String,
    pub plugins: Vec<IndexedPlugin>,
}

/// İndeksteki bir girdi — doğrulanmış ya da neden doğrulanamadığı.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogEntry {
    name: String,
    display_name: Option<String>,
    version: Option<String>,
    api: Option<u32>,
    description: Option<String>,
    files: Vec<CatalogFile>,
    /// Yalnızca girdi kurulabilirse var.
    manifest: Option<PluginManifest>,
    problem: Option<String>,
}

/// Okunmuş bir katalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    index: String,
    entries: Vec<CatalogEntry>,
}

/// İndeks dosyasının şemayı soran en küçük hâli.
#[derive(Deserialize)]
struct SchemaProbe {
    schema: Option<u32>,
}

#[derive(Deserialize)]
struct RawIndex {
    #[serde(default)]
    plugins: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct IndexOut<'a> {
    schema: u32,
    url_template: &'a str,
    plugins: Vec<EntryOut>,
}

#[derive(Serialize)]
struct EntryOut {
    manifest: serde_json::Value,
    files: Vec<CatalogFile>,
}

impl Catalog {
    /// Okunduğu adres.
    #[must_use]
    pub fn index(&self) -> &str {
        &self.index
    }

    /// Girdi adları, indeksteki sırayla.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    /// İndeks metnini okur.
    ///
    /// Bozuk bir **girdi** okumayı durdurmaz: sebebi o girdinin
    /// `problem`'ine yazılır ve gerisi okunur. Bozuk bir **indeks** (JSON
    /// değil, şeması bilinmiyor) hatadır.
    ///
    /// # Errors
    /// Metin JSON değilse, şema yoksa ya da bu çekirdeğin okuduğundan
    /// farklıysa.
    pub fn parse(index: &str, body: &[u8]) -> Result<Self> {
        let probe: SchemaProbe = serde_json::from_slice(body).map_err(|err| {
            catalog_err(
                index,
                format!("indeks JSON olarak okunamadı: {err} — adres bir indeks mi gösteriyor?"),
            )
        })?;
        match probe.schema {
            Some(INDEX_SCHEMA) => {}
            Some(schema) => {
                return Err(catalog_err(
                    index,
                    format!(
                        "indeks biçimi bu sürümün okuduğundan farklı (şema {schema}, bu sürüm \
                         {INDEX_SCHEMA} okur) — headshell'i güncelleyin"
                    ),
                ));
            }
            None => {
                return Err(catalog_err(
                    index,
                    "indekste `schema` alanı yok — bu bir headshell eklenti indeksi değil"
                        .to_owned(),
                ));
            }
        }
        let raw: RawIndex = serde_json::from_slice(body).map_err(|err| {
            catalog_err(
                index,
                format!("indeksin `plugins` listesi okunamadı: {err}"),
            )
        })?;

        let loopback = url_host(index).is_ok_and(|host| is_loopback(&host));
        let mut entries: Vec<CatalogEntry> = raw
            .plugins
            .into_iter()
            .map(|value| parse_entry(index, value, loopback))
            .collect();

        // Aynı ad iki kez: hangisinin kurulacağı tahmin edilmez (K9).
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for entry in &entries {
            *counts.entry(entry.name.clone()).or_default() += 1;
        }
        for entry in &mut entries {
            if counts.get(&entry.name).is_some_and(|count| *count > 1) {
                entry.manifest = None;
                entry.problem = Some(
                    "katalogda bu ad birden çok kez var — hangisinin kurulacağı tahmin edilmez"
                        .to_owned(),
                );
            }
        }
        Ok(Self {
            index: index.to_owned(),
            entries,
        })
    }

    fn lookup(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// Adı verilen girdi; yoksa hangi adların olduğu ve belki kastedilen.
    fn find(&self, name: &str) -> Result<&CatalogEntry> {
        if let Some(entry) = self.lookup(name) {
            return Ok(entry);
        }
        let hint = self
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| format!(" — bunu mu demek istediniz: `{}`?", entry.name))
            .unwrap_or_default();
        let available = if self.entries.is_empty() {
            "katalog boş".to_owned()
        } else {
            format!("katalogdakiler: {}", self.names().join(", "))
        };
        Err(Error::new(
            Stage::PluginCatalog,
            ErrorKind::NotFound {
                what: format!(
                    "katalogda `{name}` adlı eklenti ({}){hint}; {available}",
                    self.index
                ),
            },
        ))
    }

    /// Kataloğu bu makineye karşı okur: her girdinin kurulu olup olmadığı,
    /// güncelleme olup olmadığı, ve katalogdan çekilmiş kurulu eklentiler.
    /// **Ağa çıkmaz.**
    ///
    /// # Errors
    /// Eklenti dizini var ama okunamıyorsa.
    pub fn survey(&self, config: &Config) -> Result<CatalogSurvey> {
        let plugins: Vec<CatalogPlugin> = self
            .entries
            .iter()
            .map(|entry| entry.view(state_of(config, entry)))
            .collect();

        let mut summary = CatalogSummary {
            listed: plugins.len(),
            ..CatalogSummary::default()
        };
        for plugin in &plugins {
            if plugin.problem.is_some() {
                summary.problems += 1;
            } else {
                summary.installable += 1;
            }
            if plugin.installed.is_installed() {
                summary.installed += 1;
            }
            if plugin.problem.is_none()
                && matches!(plugin.installed, InstallState::UpdateAvailable { .. })
            {
                summary.updates += 1;
            }
        }

        Ok(CatalogSurvey {
            plugins,
            delisted: self.delisted(config)?,
            summary,
        })
    }

    /// Bu katalogdan kurulmuş ama artık listede olmayanlar.
    fn delisted(&self, config: &Config) -> Result<Vec<String>> {
        let mut delisted = Vec::new();
        for (name, dir) in installed_dirs(config)? {
            if self.lookup(&name).is_some() {
                continue;
            }
            match read_record(&dir) {
                Ok(Some(record)) if record.index == self.index => delisted.push(name),
                Ok(_) => {}
                Err(err) => tracing::warn!(
                    plugin = %name,
                    error = %err.chain_text().replace('\n', " "),
                    "köken kaydı okunamadı; katalogdan çekilip çekilmediği söylenemiyor"
                ),
            }
        }
        Ok(delisted)
    }

    /// Güncellemede bakılacak eklentiler: katalogdaki adlardan bu makinede
    /// kurulu olanlar ve bu katalogdan kurulup listeden çekilenler.
    ///
    /// Elle kurulmuş ve katalogda adı olmayan bir eklenti (bir geliştiricinin
    /// çalışma kopyası) listeye girmez — katalogla ilgisi yok.
    ///
    /// # Errors
    /// Eklenti dizini var ama okunamıyorsa.
    pub fn update_candidates(&self, config: &Config) -> Result<Vec<String>> {
        let mut names: BTreeSet<String> = installed_dirs(config)?
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| self.lookup(name).is_some())
            .collect();
        names.extend(self.delisted(config)?);
        Ok(names.into_iter().collect())
    }
}

impl CatalogEntry {
    fn view(&self, installed: InstallState) -> CatalogPlugin {
        let manifest = self.manifest.as_ref();
        CatalogPlugin {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            version: self.version.clone(),
            api: self.api,
            description: self.description.clone(),
            capabilities: manifest.map(|m| m.capabilities.clone()).unwrap_or_default(),
            permissions: manifest.map(|m| m.permissions.clone()).unwrap_or_default(),
            requires: manifest.map(|m| m.requires.clone()).unwrap_or_default(),
            files: self.files.clone(),
            problem: self.problem.clone(),
            installed,
        }
    }

    /// Kurulabilir manifest; değilse sebebiyle hata.
    fn installable(&self, index: &str) -> Result<&PluginManifest> {
        match (&self.manifest, &self.problem) {
            (Some(manifest), None) => Ok(manifest),
            (_, Some(problem)) => Err(catalog_err(
                index,
                format!("{} kurulamıyor: {problem}", self.name),
            )),
            (None, None) => Err(catalog_err(
                index,
                format!("{} kurulamıyor: girdi doğrulanmadı", self.name),
            )),
        }
    }
}

/// İndeksteki bir girdiyi okur. Hiç düşmez: sorun `problem`'e yazılır.
fn parse_entry(index: &str, value: serde_json::Value, loopback: bool) -> CatalogEntry {
    let manifest_value = value.get("manifest").cloned();
    let field = |key: &str| {
        manifest_value
            .as_ref()
            .and_then(|manifest| manifest.get(key))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let api = manifest_value
        .as_ref()
        .and_then(|manifest| manifest.get("api"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|api| u32::try_from(api).ok());
    let files: std::result::Result<Vec<CatalogFile>, String> = value
        .get("files")
        .cloned()
        .ok_or_else(|| "`files` yok".to_owned())
        .and_then(|files| {
            serde_json::from_value::<Vec<CatalogFile>>(files)
                .map_err(|err| format!("`files` okunamadı: {err}"))
        })
        .map(|files| {
            files
                .into_iter()
                .map(|file| CatalogFile {
                    sha256: file.sha256.trim().to_ascii_lowercase(),
                    ..file
                })
                .collect()
        });

    let mut entry = CatalogEntry {
        name: field("name").unwrap_or_default(),
        display_name: field("display_name"),
        version: field("version"),
        api,
        description: field("description"),
        files: files.clone().unwrap_or_default(),
        manifest: None,
        problem: None,
    };

    let checked = (|| -> std::result::Result<PluginManifest, String> {
        let manifest_value = manifest_value.ok_or_else(|| "`manifest` yok".to_owned())?;
        validate_catalog_name(&entry.name)?;
        if let Some(api) = entry.api
            && api != PLUGIN_API
        {
            return Err(format!(
                "bu headshell sürümüyle uyumsuz: eklenti api {api}, çekirdek api {PLUGIN_API}"
            ));
        }
        let origin = PathBuf::from(format!("{index}#{}", entry.name));
        let manifest = PluginManifest::parse(&manifest_value.to_string(), &entry.name, &origin)
            .map_err(|err| cause_text(&err))?;
        let version = manifest
            .version
            .as_deref()
            .ok_or_else(|| "`version` yok — katalogdaki her eklenti sürümlü olmalı".to_owned())?;
        validate_version(version)?;
        validate_files(&files?, &manifest, loopback)?;
        Ok(manifest)
    })();
    match checked {
        Ok(manifest) => entry.manifest = Some(manifest),
        Err(problem) => entry.problem = Some(problem),
    }
    entry
}

/// Girdinin dosya listesi: tam olarak `plugin.json` ve betik, her biri
/// geçerli bir yol, karma ve adresle.
fn validate_files(
    files: &[CatalogFile],
    manifest: &PluginManifest,
    loopback: bool,
) -> std::result::Result<(), String> {
    let mut seen = BTreeSet::new();
    for file in files {
        validate_file_path(&file.path)?;
        if !seen.insert(file.path.as_str()) {
            return Err(format!("`{}` dosya listesinde iki kez var", file.path));
        }
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "`{}`: `sha256` 64 haneli onaltılık olmalı (bulunan: {} hane)",
                file.path,
                file.sha256.len()
            ));
        }
        check_url(&file.url, loopback).map_err(|detail| format!("`{}`: {detail}", file.path))?;
    }
    let main = manifest.main_file();
    let expected: BTreeSet<&str> = [MANIFEST_FILE, main.as_str()].into_iter().collect();
    if seen != expected {
        return Err(format!(
            "dosya listesi tam olarak `{}` olmalı; bulunan: `{}`",
            expected.into_iter().collect::<Vec<_>>().join("`, `"),
            seen.into_iter().collect::<Vec<_>>().join("`, `")
        ));
    }
    Ok(())
}

/// Katalogdaki bir dosya yolu eklenti dizininin içinde, düz ve adrese
/// yüzde kodlamasız girebilen bir yol mu.
fn validate_file_path(path: &str) -> std::result::Result<(), String> {
    let invalid = || {
        Err(format!(
            "`{path}` geçerli bir eklenti dosyası yolu değil: göreli, `/` ayırıcılı, her parçası \
             ASCII harf/rakam/`.`/`-`/`_` ve noktayla başlamayan bir yol olmalı"
        ))
    };
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return invalid();
    }
    for segment in path.split('/') {
        let valid = !segment.is_empty()
            && !segment.starts_with('.')
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !valid {
            return invalid();
        }
    }
    let top = path.split('/').next().unwrap_or(path);
    if RESERVED.contains(&top) {
        return Err(format!(
            "`{path}`: `{top}` motorun eklenti dizininde kullandığı bir ad"
        ));
    }
    Ok(())
}

/// Katalogdaki bir sürüm adresin içine girer; yüzde kodlaması gerekmemeli.
fn validate_version(version: &str) -> std::result::Result<(), String> {
    let valid = !version.is_empty()
        && version.len() <= 64
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "sürüm `{version}` katalogda kullanılamaz: 1–64 karakter, ASCII harf/rakam/`.`/`-`/`_`/`+`"
        ))
    }
}

/// Bir adres katalog için kabul edilir mi: `https://`, ya da düz `http://`
/// yalnızca bu makinenin kendisine (`127.0.0.1`, `localhost`) ve yalnızca
/// indeks de oradaysa.
///
/// İndeks güvenin köküdür — dosyaların karmasını o taşır. Düz HTTP'den gelen
/// bir indeksi yoldaki herkes değiştirebilir; yerel döngü adresinde bu yol
/// yok, sınamalar ve yerel aynalar orada koşar.
fn check_url(url: &str, loopback_allowed: bool) -> std::result::Result<(), String> {
    let host = url_host(url)?;
    let https = url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    if https || (loopback_allowed && is_loopback(&host)) {
        Ok(())
    } else {
        Err(format!(
            "yalnızca https:// adresleri kabul edilir (düz http yalnızca bu makinenin kendisi \
             için: 127.0.0.1, localhost) — bulunan: {url}"
        ))
    }
}

fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "localhost"
}

/// Hatanın aşama satırı olmadan nedeni: bir girdinin `problem`'ine yazılır,
/// `ADIM: …` orada gürültü.
fn cause_text(err: &Error) -> String {
    let mut parts = Vec::new();
    let mut current: Option<&dyn std::error::Error> = Some(err.kind());
    while let Some(cause) = current {
        parts.push(cause.to_string());
        current = cause.source();
    }
    parts.join(": ")
}

fn catalog_err(index: &str, detail: String) -> Error {
    Error::new(
        Stage::PluginCatalog,
        ErrorKind::PluginCatalog {
            index: index.to_owned(),
            detail,
        },
    )
}

/// Kataloğu okur.
///
/// # Errors
/// Adres kabul edilmezse, ağa ulaşılamazsa (`NETWORK_REQUEST`), indeks
/// yoksa ya da okunamıyorsa (`PLUGIN_CATALOG`).
pub async fn fetch(http: &dyn HttpClient, index: &str) -> Result<Catalog> {
    check_url(index, true).map_err(|detail| catalog_err(index, detail))?;
    let request = HttpRequest::get(index);
    let response = http.send(&request).await?;
    if response.status == 404 || response.status == 410 {
        return Err(catalog_err(
            index,
            format!(
                "indeks bulunamadı (HTTP {}) — adres doğru mu? `{INDEX_ENV}` verilmişse onu \
                 denetleyin",
                response.status
            ),
        ));
    }
    response.error_for_status(index)?;
    if response.body.len() > MAX_INDEX_BYTES {
        return Err(catalog_err(
            index,
            format!(
                "indeks {} bayt, sınır {MAX_INDEX_BYTES} — adres bir indeks göstermiyor olabilir",
                response.body.len()
            ),
        ));
    }
    Catalog::parse(index, &response.body)
}

/// Bir eklentiyi katalogdan kurar: indirir, karmalarını ve manifestini
/// doğrular, köken kaydıyla birlikte **tek adımda** yerine koyar.
///
/// Dosyalar önce veri dizininde geçici bir dizine yazılır ve dizin olarak
/// taşınır: yarıda kalan bir kurulum eklenti dizininde yarım bir eklenti
/// bırakmaz. Eklenti kurulduktan sonra **onay bekler** (D-040).
///
/// # Errors
/// Eklenti katalogda yoksa ya da kurulamıyorsa, bu adla zaten bir dizin
/// varsa, indirme ya da karma tutmazsa, dosyalar yazılamazsa.
pub async fn install(
    config: &Config,
    http: &dyn HttpClient,
    catalog: &Catalog,
    name: &str,
) -> Result<CatalogFetch> {
    let entry = catalog.find(name)?;
    let manifest = entry.installable(&catalog.index)?;
    let dir = config.plugins_dir().join(&entry.name);
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(catalog_err(
            &catalog.index,
            format!(
                "{} zaten kurulu ({}) — güncellemek için `headshell plugin update {}`",
                entry.name,
                dir.display(),
                entry.name
            ),
        ));
    }
    let version = manifest.version.clone().unwrap_or_default();

    let files = download(http, &catalog.index, entry).await?;
    verify_manifest(&catalog.index, entry, manifest, &files)?;

    let plugins_dir = config.plugins_dir();
    std::fs::create_dir_all(&plugins_dir)
        .map_err(|err| io_err(Stage::PluginCatalog, &plugins_dir, err))?;
    let staging = Staging::create(config.data_dir(), &entry.name)?;
    for (file, bytes) in &files {
        write_file(&relative(staging.path(), &file.path), bytes)?;
    }
    write_record(
        staging.path(),
        &InstallRecord {
            index: catalog.index.clone(),
            version: version.clone(),
            installed_at: jiff::Timestamp::now(),
            files: record_files(&files),
        },
    )?;
    staging.move_to(&dir, &catalog.index)?;

    tracing::info!(eklenti = %entry.name, surum = %version, katalog = %catalog.index, "eklenti katalogdan kuruldu");
    Ok(CatalogFetch {
        index: catalog.index.clone(),
        version,
        files: entry.files.clone(),
    })
}

/// Katalogdan kurulmuş bir eklentiyi katalogdaki sürüme getirir.
///
/// Yalnızca köken kaydı olan ve dosyaları kayıtla aynı olan eklentiye
/// dokunur; ötekiler için sebebi yazan [`UpdateOutcome::Skipped`] döner.
/// Dosyalar tek tek, her biri atomik olarak değiştirilir ve köken kaydı en
/// son yazılır; `state/` (eklentinin kalıcı deposu) korunur.
///
/// `platform` araç değişikliklerinin hangi platformun ikilisine göre
/// söyleneceği ([`super::artifact::current_platform`]).
///
/// # Errors
/// Eklenti kurulu değilse, indirme ya da karma tutmazsa, dosyalar
/// yazılamazsa. Yarıda kalan bir güncelleme bir sonrakinde tamamlanır:
/// katalogdaki yeni karmayı taşıyan dosya "yerel değişiklik" sayılmaz.
pub async fn update(
    config: &Config,
    http: &dyn HttpClient,
    catalog: &Catalog,
    name: &str,
    platform: &str,
) -> Result<UpdateOutcome> {
    let dir = installed_dir(config, name)?;
    let Some(entry) = catalog.lookup(name) else {
        return Ok(UpdateOutcome::Skipped {
            reason: format!(
                "katalogda yok ({}) — katalogdan çekilmiş olabilir",
                catalog.index
            ),
        });
    };
    let (installed, available) = match state_of(config, entry) {
        InstallState::UpdateAvailable {
            installed,
            available,
        } => (installed, available),
        InstallState::Current { version } => return Ok(UpdateOutcome::Current { version }),
        InstallState::Manual => {
            return Ok(UpdateOutcome::Skipped {
                reason: format!(
                    "elle kurulmuş (köken kaydı yok) — güncelleme elle konan dosyaların üstüne \
                     yazmaz; katalogdakini kurmak için önce `headshell plugin remove {name}`"
                ),
            });
        }
        InstallState::Modified { files, .. } => {
            return Ok(UpdateOutcome::Skipped {
                reason: format!(
                    "yerelde değiştirilmiş ({}) — üstüne yazılmaz; katalogdakine dönmek için \
                     `headshell plugin remove {name}` ve `install`",
                    files.join(", ")
                ),
            });
        }
        InstallState::Unreadable { detail } => {
            return Ok(UpdateOutcome::Skipped {
                reason: format!("köken kaydı okunamadı: {detail}"),
            });
        }
        InstallState::NotInstalled => {
            return Ok(UpdateOutcome::Skipped {
                reason: "kurulu değil".to_owned(),
            });
        }
    };
    if let Some(problem) = &entry.problem {
        return Ok(UpdateOutcome::Skipped {
            reason: format!("katalogdaki sürüm ({available}) kurulamıyor: {problem}"),
        });
    }
    let manifest = entry.installable(&catalog.index)?;
    let previous = read_record(&dir)?;
    // Eski manifest okunamıyorsa güncelleme yine yapılır (bozuk bir sürümü
    // düzeltmenin yolu bu); karşılaştırma boş bir öncülle yapılır.
    let old_manifest = PluginManifest::load(&dir).ok();

    let files = download(http, &catalog.index, entry).await?;
    verify_manifest(&catalog.index, entry, manifest, &files)?;

    // Betik önce, manifest sonra, köken kaydı en son: yarıda kalırsa eski
    // manifestin izinleriyle koşan yeni betik ancak eski izinlerin içinde
    // kalabilir, ve eski kayıt bir sonraki güncellemeyi tetikler.
    let mut ordered: Vec<&(CatalogFile, Vec<u8>)> = files.iter().collect();
    ordered.sort_by_key(|(file, _)| file.path == MANIFEST_FILE);
    for (file, bytes) in ordered {
        replace_file(&dir, &file.path, bytes)?;
    }
    if let Some(previous) = &previous {
        for path in previous.files.keys() {
            if files.iter().any(|(file, _)| &file.path == path) {
                continue;
            }
            let stale = relative(&dir, path);
            match std::fs::remove_file(&stale) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(io_err(Stage::PluginCatalog, &stale, err)),
            }
        }
    }
    write_record(
        &dir,
        &InstallRecord {
            index: catalog.index.clone(),
            version: available.clone(),
            installed_at: jiff::Timestamp::now(),
            files: record_files(&files),
        },
    )?;

    let (old_permissions, old_requires) = old_manifest
        .map(|old| (old.permissions, old.requires))
        .unwrap_or_default();
    tracing::info!(eklenti = %name, onceki = %installed, yeni = %available, "eklenti güncellendi");
    Ok(UpdateOutcome::Updated {
        from: installed,
        to: available,
        permissions_added: manifest.permissions.beyond(&old_permissions),
        tools_changed: tool_changes(&old_requires, &manifest.requires, platform),
    })
}

/// Bir eklentiyi diskten kaldırır: dizini, içindeki `state/` ile birlikte.
///
/// Dizin bir sembolik bağlantıysa **yalnızca bağlantı** kaldırılır, hedefine
/// dokunulmaz. Onay kaydı ve sırlar burada değil, çağıranda ([`crate::session`]).
///
/// # Errors
/// Ad tek bir dizin adı değilse, eklenti kurulu değilse ya da silinemezse.
pub fn remove(config: &Config, name: &str) -> Result<Removed> {
    let dir = installed_dir(config, name)?;
    let meta =
        std::fs::symlink_metadata(&dir).map_err(|err| io_err(Stage::PluginCatalog, &dir, err))?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(&dir).ok();
        remove_link(&dir)?;
        return Ok(Removed {
            path: dir,
            link_target: target,
        });
    }
    if !meta.is_dir() {
        return Err(Error::new(
            Stage::PluginCatalog,
            ErrorKind::InvalidInput {
                detail: format!("{} bir eklenti dizini değil", dir.display()),
            },
        ));
    }
    std::fs::remove_dir_all(&dir).map_err(|err| io_err(Stage::PluginCatalog, &dir, err))?;
    Ok(Removed {
        path: dir,
        link_target: None,
    })
}

/// Kurulu bir eklentinin dizini.
///
/// Ad yola eklenmeden önce doğrulanır ([`validate_local_name`]): `../`
/// taşıyan bir ad veri dizininin dışına uzanırdı. Kurulu değilse hata
/// **ne yapılacağını** söyler.
///
/// # Errors
/// Ad tek bir dizin adı değilse ya da bu adla bir eklenti yoksa.
pub fn installed_dir(config: &Config, name: &str) -> Result<PathBuf> {
    validate_local_name(name)
        .map_err(|detail| Error::new(Stage::PluginCatalog, ErrorKind::InvalidInput { detail }))?;
    let dir = config.plugins_dir().join(name);
    match std::fs::symlink_metadata(&dir) {
        Ok(_) => Ok(dir),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(Error::new(
            Stage::PluginCatalog,
            ErrorKind::NotFound {
                what: format!(
                    "kurulu eklenti: {name} ({}) — kurmak için `headshell plugin install {name}`",
                    dir.display()
                ),
            },
        )),
        Err(err) => Err(io_err(Stage::PluginCatalog, &dir, err)),
    }
}

/// Bir sembolik bağlantıyı kaldırır. Unix'te bağlantı bir dosyadır;
/// Windows'ta bir dizin bağlantısı `remove_dir` ister. İkisi de hedefe
/// dokunmaz.
fn remove_link(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(first) => {
            std::fs::remove_dir(path).map_err(|_| io_err(Stage::PluginCatalog, path, first))
        }
    }
}

/// Bir eklentinin köken kaydını okur. Kayıt **yoksa** `None`: eklenti elle
/// konmuş demektir, hata değil.
///
/// # Errors
/// Kayıt var ama okunamıyorsa ya da bozuksa.
pub fn read_record(dir: &Path) -> Result<Option<InstallRecord>> {
    let path = dir.join(ORIGIN_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(io_err(Stage::PluginCatalog, &path, err)),
    };
    serde_json::from_str(&raw).map(Some).map_err(|source| {
        Error::new(
            Stage::PluginCatalog,
            ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })
}

/// Bir katalog girdisinin bu makinedeki durumu. Ağa çıkmaz.
fn state_of(config: &Config, entry: &CatalogEntry) -> InstallState {
    let dir = config.plugins_dir().join(&entry.name);
    match std::fs::symlink_metadata(&dir) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return InstallState::NotInstalled;
        }
        Err(err) => {
            return InstallState::Unreadable {
                detail: format!("{}: {err}", dir.display()),
            };
        }
    }
    let record = match read_record(&dir) {
        Ok(Some(record)) => record,
        Ok(None) => return InstallState::Manual,
        Err(err) => {
            return InstallState::Unreadable {
                detail: cause_text(&err),
            };
        }
    };
    let modified = modified_files(&dir, &record, &entry.files);
    if !modified.is_empty() {
        return InstallState::Modified {
            version: record.version,
            files: modified,
        };
    }
    match &entry.version {
        Some(available) if *available != record.version => InstallState::UpdateAvailable {
            installed: record.version,
            available: available.clone(),
        },
        _ => InstallState::Current {
            version: record.version,
        },
    }
}

/// Kurulduğu gibi olmayan dosyalar. Katalogdaki **yeni** karmayı taşıyan
/// dosya değişmiş sayılmaz: yarıda kalmış bir güncellemenin izidir ve bir
/// sonraki güncelleme onu tamamlar.
fn modified_files(
    dir: &Path,
    record: &InstallRecord,
    catalog_files: &[CatalogFile],
) -> Vec<String> {
    record
        .files
        .iter()
        .filter(|(path, recorded)| match hash_file(&relative(dir, path)) {
            Ok(found) => {
                found != **recorded
                    && !catalog_files
                        .iter()
                        .any(|file| &file.path == *path && file.sha256 == found)
            }
            // Silinmiş ya da okunamayan dosya da kurulduğu gibi değil.
            Err(_) => true,
        })
        .map(|(path, _)| path.clone())
        .collect()
}

/// Eklenti dizinindeki alt dizinler: `(ad, yol)`. Dizin yoksa boş.
fn installed_dirs(config: &Config) -> Result<Vec<(String, PathBuf)>> {
    let plugins_dir = config.plugins_dir();
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(io_err(Stage::PluginCatalog, &plugins_dir, err)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| io_err(Stage::PluginCatalog, &plugins_dir, err))?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push((entry.file_name().to_string_lossy().into_owned(), path));
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Girdinin dosyalarını indirir ve **hepsinin** karmasını doğrular. Biri
/// tutmazsa hiçbir şey dönmez — diske de hiçbir şey yazılmaz.
async fn download(
    http: &dyn HttpClient,
    index: &str,
    entry: &CatalogEntry,
) -> Result<Vec<(CatalogFile, Vec<u8>)>> {
    let mut out = Vec::with_capacity(entry.files.len());
    for file in &entry.files {
        let request = HttpRequest::get(&file.url);
        let response = http.send(&request).await?;
        if response.status == 404 || response.status == 410 {
            return Err(catalog_err(
                index,
                format!(
                    "{} {}: dosya bulunamadı (HTTP {}, {}) — indeks var olmayan bir sürümü \
                     gösteriyor; düzeltmesi katalog bakımcısının işi",
                    entry.name, file.path, response.status, file.url
                ),
            ));
        }
        response.error_for_status(&file.url)?;
        if response.body.len() > MAX_FILE_BYTES {
            return Err(catalog_err(
                index,
                format!(
                    "{} {}: {} bayt, sınır {MAX_FILE_BYTES}",
                    entry.name,
                    file.path,
                    response.body.len()
                ),
            ));
        }
        let found = sha256_hex(&response.body);
        if found != file.sha256 {
            return Err(catalog_err(
                index,
                format!(
                    "{} {}: karma tutmuyor (beklenen {}…, inen {}…) — hiçbir şey yazılmadı",
                    entry.name,
                    file.path,
                    short_hash(&file.sha256),
                    short_hash(&found)
                ),
            ));
        }
        out.push((file.clone(), response.body));
    }
    Ok(out)
}

/// İnen `plugin.json` indeksin gösterdiği manifestle aynı mı: katalogda
/// gösterilen izinler kurulanın izinleri olmalı.
fn verify_manifest(
    index: &str,
    entry: &CatalogEntry,
    expected: &PluginManifest,
    files: &[(CatalogFile, Vec<u8>)],
) -> Result<()> {
    let Some((file, bytes)) = files.iter().find(|(file, _)| file.path == MANIFEST_FILE) else {
        return Err(catalog_err(
            index,
            format!("{}: `{MANIFEST_FILE}` indirilmedi", entry.name),
        ));
    };
    let raw = std::str::from_utf8(bytes).map_err(|err| {
        catalog_err(
            index,
            format!("{} {MANIFEST_FILE} UTF-8 değil: {err}", entry.name),
        )
    })?;
    let downloaded = PluginManifest::parse(raw, &entry.name, Path::new(&file.url))
        .map_err(|err| catalog_err(index, cause_text(&err)))?;
    if downloaded != *expected {
        return Err(catalog_err(
            index,
            format!(
                "{}: inen {MANIFEST_FILE} indeksin gösterdiğinden farklı — katalogda gösterilen \
                 izinler kurulanın izinleri olmalı; kurulmadı",
                entry.name
            ),
        ));
    }
    Ok(())
}

/// Motorun kurduğu araçlardaki değişiklikler, bu platformun ikilisine göre.
fn tool_changes(old: &[Requirement], new: &[Requirement], platform: &str) -> Vec<ToolChange> {
    let names: BTreeSet<&str> = old
        .iter()
        .chain(new)
        .map(|requirement| requirement.name.as_str())
        .collect();
    let asset = |requirement: Option<&Requirement>| {
        requirement
            .and_then(|requirement| requirement.asset_for(platform))
            .map(|asset| (asset.url.clone(), asset.sha256.trim().to_ascii_lowercase()))
    };
    names
        .into_iter()
        .filter_map(|name| {
            let before = old.iter().find(|requirement| requirement.name == name);
            let after = new.iter().find(|requirement| requirement.name == name);
            let binary_changed = asset(before) != asset(after);
            let from = before.map(|requirement| requirement.version.clone());
            let to = after.map(|requirement| requirement.version.clone());
            (binary_changed || from != to).then(|| ToolChange {
                name: name.to_owned(),
                from,
                to,
                binary_changed,
            })
        })
        .collect()
}

fn record_files(files: &[(CatalogFile, Vec<u8>)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(file, _)| (file.path.clone(), file.sha256.clone()))
        .collect()
}

/// `/` ayırıcılı göreli yolu dizine ekler — her platformun kendi ayırıcısıyla.
fn relative(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_path_buf(), |acc, segment| acc.join(segment))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_err(Stage::PluginCatalog, parent, err))?;
    }
    std::fs::write(path, bytes).map_err(|err| io_err(Stage::PluginCatalog, path, err))
}

/// Bir dosyayı atomik olarak değiştirir: yanına koşuma özgü geçici bir
/// adla yazar (D-060), sonra üstüne taşır.
fn replace_file(dir: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let target = relative(dir, path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_err(Stage::PluginCatalog, parent, err))?;
    }
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(
        "{file_name}.{}-{}.indiriliyor",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    ));
    write_file(&temp, bytes)?;
    if let Err(err) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(io_err(Stage::PluginCatalog, &target, err));
    }
    Ok(())
}

fn write_record(dir: &Path, record: &InstallRecord) -> Result<()> {
    let path = dir.join(ORIGIN_FILE);
    let mut text = serde_json::to_string_pretty(record).map_err(|source| {
        Error::new(
            Stage::PluginCatalog,
            ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })?;
    text.push('\n');
    replace_file(dir, ORIGIN_FILE, text.as_bytes())
}

/// Kurulumun hazırlandığı geçici dizin: veri dizininde, eklenti dizininin
/// **dışında** — hazırlanırken keşif onu yarım bir eklenti sanmasın.
/// Taşınmadan düşerse (hata, panik) kendini siler.
struct Staging {
    path: PathBuf,
    moved: bool,
}

impl Staging {
    fn create(data_dir: &Path, name: &str) -> Result<Self> {
        let path = data_dir.join(format!(
            ".plugin-{name}-{}-{}.kuruluyor",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&path).map_err(|err| io_err(Stage::PluginCatalog, &path, err))?;
        Ok(Self { path, moved: false })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Hazır dizini yerine taşır. Hedef bu arada belirdiyse (aynı anda iki
    /// kurulum) üstüne yazılmaz ve bu söylenir.
    fn move_to(mut self, target: &Path, index: &str) -> Result<()> {
        if std::fs::symlink_metadata(target).is_ok() {
            return Err(catalog_err(
                index,
                format!(
                    "{} kurulurken başka bir kurulum araya girdi; üstüne yazılmadı",
                    target.display()
                ),
            ));
        }
        std::fs::rename(&self.path, target)
            .map_err(|err| io_err(Stage::PluginCatalog, target, err))?;
        self.moved = true;
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.moved {
            return;
        }
        if let Err(err) = std::fs::remove_dir_all(&self.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(yol = %self.path.display(), error = %err, "geçici kurulum dizini silinemedi");
        }
    }
}

/// Katalog deposundaki eklentilerden indeksi üretir (D-071).
///
/// Her alt dizin bir eklenti: `<ad>/plugin.json` + betik. Noktayla başlayan
/// dizinler (`.git`, `.github`) ve `plugin.json`'u olmayanlar atlanır.
/// Manifest **çekirdeğin kendi** doğrulamasından geçer — kurulumda
/// uygulanacak kuralın aynısı; indekse yalnızca bu sürümün kurabileceği
/// eklenti girer.
///
/// `url_template` her dosyanın adresini üretir: `{name}`, `{version}` ve
/// `{path}` zorunlu. `{version}` zorunlu çünkü adres sürüme sabitlenmeli
/// (modül belgesi, "Güven").
///
/// Çıktı belirlenimci: aynı dizin her zaman aynı metni üretir, ki
/// `--check` bir fark gördüğünde gerçekten bir şey değişmiş olsun.
///
/// # Errors
/// Şablon geçersizse, dizin okunamazsa, hiç eklenti yoksa ya da **herhangi
/// bir** eklenti geçersizse — hepsi birden, tek tek sebebiyle.
pub fn build_index(dir: &Path, url_template: &str) -> Result<BuiltIndex> {
    let origin = dir.display().to_string();
    validate_template(url_template).map_err(|detail| catalog_err(&origin, detail))?;

    let entries = std::fs::read_dir(dir).map_err(|err| io_err(Stage::PluginCatalog, dir, err))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| io_err(Stage::PluginCatalog, dir, err))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with('.') || !path.is_dir() || !path.join(MANIFEST_FILE).exists() {
            continue;
        }
        candidates.push((name, path));
    }
    candidates.sort();
    if candidates.is_empty() {
        return Err(catalog_err(
            &origin,
            format!(
                "dizinde eklenti yok — her eklenti `<ad>/{MANIFEST_FILE}` olarak durmalı; boş bir \
                 indeks yayımlanırsa kataloğu siler"
            ),
        ));
    }

    let mut out = Vec::new();
    let mut plugins = Vec::new();
    let mut problems = Vec::new();
    for (name, path) in &candidates {
        match index_one(name, path, url_template) {
            Ok((entry, indexed)) => {
                out.push(entry);
                plugins.push(indexed);
            }
            Err(problem) => problems.push(format!("{name}: {problem}")),
        }
    }
    if !problems.is_empty() {
        return Err(catalog_err(
            &origin,
            format!(
                "{} eklenti indekse giremedi:\n  {}",
                problems.len(),
                problems.join("\n  ")
            ),
        ));
    }

    let mut json = serde_json::to_string_pretty(&IndexOut {
        schema: INDEX_SCHEMA,
        url_template,
        plugins: out,
    })
    .map_err(|err| catalog_err(&origin, format!("indeks yazılamadı: {err}")))?;
    json.push('\n');
    Ok(BuiltIndex { json, plugins })
}

fn index_one(
    name: &str,
    dir: &Path,
    url_template: &str,
) -> std::result::Result<(EntryOut, IndexedPlugin), String> {
    validate_catalog_name(name)?;
    let manifest = PluginManifest::load(dir).map_err(|err| cause_text(&err))?;
    let version = manifest
        .version
        .clone()
        .ok_or_else(|| "`version` yok — katalogdaki her eklenti sürümlü olmalı".to_owned())?;
    validate_version(&version)?;

    let manifest_path = dir.join(MANIFEST_FILE);
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("{}: {err}", manifest_path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| format!("{}: {err}", manifest_path.display()))?;

    let mut files = Vec::new();
    for path in [MANIFEST_FILE.to_owned(), manifest.main_file()] {
        validate_file_path(&path)?;
        let local = relative(dir, &path);
        let bytes = std::fs::read(&local).map_err(|err| format!("{}: {err}", local.display()))?;
        files.push(CatalogFile {
            url: url_template
                .replace("{name}", name)
                .replace("{version}", &version)
                .replace("{path}", &path),
            path,
            sha256: sha256_hex(&bytes),
        });
    }
    Ok((
        EntryOut {
            manifest: value,
            files: files.clone(),
        },
        IndexedPlugin {
            name: name.to_owned(),
            version,
            files,
        },
    ))
}

fn validate_template(template: &str) -> std::result::Result<(), String> {
    for placeholder in ["{name}", "{version}", "{path}"] {
        if !template.contains(placeholder) {
            return Err(format!(
                "adres şablonunda `{placeholder}` yok — her sürümün her dosyası kendi adresine \
                 gitmeli (şablon: {template})"
            ));
        }
    }
    let sample = template
        .replace("{name}", "ornek")
        .replace("{version}", "1.0.0")
        .replace("{path}", "main.js");
    check_url(&sample, true).map_err(|detail| format!("adres şablonu: {detail}"))
}

/// İndeksi katalog deposunun köküne atomik olarak yazar ([`INDEX_FILE`]).
///
/// # Errors
/// Dosya yazılamazsa.
pub fn write_index(dir: &Path, json: &str) -> Result<PathBuf> {
    replace_file(dir, INDEX_FILE, json.as_bytes())?;
    Ok(dir.join(INDEX_FILE))
}

/// Var olan bir indeksin adres şablonu — `plugin index` şablon verilmezse
/// onu kullanır, ki her üretim aynı adresleri yazsın. Dosya yoksa `None`.
///
/// # Errors
/// Dosya var ama okunamıyorsa ya da JSON değilse.
pub fn read_url_template(index_path: &Path) -> Result<Option<String>> {
    let raw = match std::fs::read_to_string(index_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(io_err(Stage::PluginCatalog, index_path, err)),
    };
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|source| {
        Error::new(
            Stage::PluginCatalog,
            ErrorKind::Json {
                entry: index_path.display().to_string(),
                source,
            },
        )
    })?;
    Ok(value
        .get("url_template")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}

/// İki indeks metni arasında hangi eklentilerin farklı olduğu — `--check`
/// "güncel değil" derken **neyin** güncel olmadığını söylesin (K9).
#[must_use]
pub fn index_differences(existing: &str, built: &str) -> Vec<String> {
    fn by_name(text: &str) -> Option<BTreeMap<String, serde_json::Value>> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let plugins = value.get("plugins")?.as_array()?;
        Some(
            plugins
                .iter()
                .map(|entry| {
                    let name = entry
                        .get("manifest")
                        .and_then(|manifest| manifest.get("name"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    (name, entry.clone())
                })
                .collect(),
        )
    }
    let (Some(old), Some(new)) = (by_name(existing), by_name(built)) else {
        return vec!["var olan index.json okunamadı".to_owned()];
    };
    let mut differences = Vec::new();
    for (name, entry) in &new {
        match old.get(name) {
            None => differences.push(format!("{name}: indekste yok")),
            Some(previous) if previous != entry => {
                differences.push(format!("{name}: girdisi değişti"));
            }
            Some(_) => {}
        }
    }
    for name in old.keys() {
        if !new.contains_key(name) {
            differences.push(format!("{name}: indekste var ama dizini yok"));
        }
    }
    if differences.is_empty() {
        differences
            .push("eklentiler aynı; indeksin geri kalanı (şema, şablon, biçim) farklı".to_owned());
    }
    differences
}

#[cfg(test)]
mod tests;
