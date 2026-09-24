//! Motorun eser tarafı: eklentilerin beyan ettiği araçları kurmak (D-055, D-069).
//!
//! D-049 kuralı koydu — **hiçbir eklenti root yetkisi ya da sistem çapında
//! kurulum isteyemez.** D-055 bunu "eklenti sabitlenmiş bir eser beyan eder,
//! motor indirir ve karmasını doğrular" diye uyguladı. D-069 iki şeyi
//! değiştirdi:
//!
//! 1. **Python yok.** api 1'de eser yt-dlp'nin zipapp'iydi ve çalışmak için
//!    sistemin Python'unu istiyordu — "kime göndersem bir sorun çıktı"nın
//!    kaynağı buydu. api 2'de eser **platform başına** beyan edilir ve her
//!    platform için kendi kendine yeten ikili seçilir (yt-dlp'nin
//!    PyInstaller derlemeleri kendi Python'unu içinde taşıyor).
//! 2. **Akarak indirme.** Kendi kendine yeten ikili ~40 MB; HTTP istemcisinin
//!    gövdeyi belleğe alan yolu (32 MB tavan) ona yetmiyor. İndirme artık
//!    diske akıyor ve karma akarken hesaplanıyor ([`ArtifactSource`]).
//!
//! ## Platform anahtarı
//!
//! `<işletim sistemi>-<mimari>[-musl]`, iki parça da Rust'ın
//! `std::env::consts` adları: `linux-x86_64`, `macos-aarch64`,
//! `windows-x86`. Anahtar **çekirdeğin derlendiği hedeften** gelir, çalışma
//! anında sistem yoklanmaz: çekirdek bir `musl` derlemesiyse musl ikilisi
//! ister, çünkü o makinede glibc ikilisinin çalışacağı zaten bilinmiyor.
//!
//! ## Dört ayrı tanı, ve beşincisi (K9)
//!
//! Bir eserin "hazır olmaması" tek bir şey değildir:
//!
//! - [`RequirementState::Missing`] — hiç kurulmadı. `headshell plugin install`.
//! - [`RequirementState::Corrupt`] — diskte var, karması tutmuyor.
//! - [`RequirementState::Unsupported`] — bu platform için yayın **yok**.
//!   Kurulum düzeltmez; eklenti yazarının manifestine o platformu eklemesi
//!   gerekir, ya da eserin kendisi o platformu desteklemiyordur.
//! - **Kurulamadı** — ağa çıkılamadı. Yarın tekrar dene.
//! - **Yetim** — kaynak 404/410 dedi. Düzeltmek eklenti yazarının işi.
//!
//! Yetim durumu **diske yazılmıyor**: bir GitHub kesintisi 5xx döndürür ama
//! yazılsaydı tek bir kötü an bir eklentiyi kalıcı olarak damgalardı.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::diag::Stage;
use crate::error::{Result, io_err};
use crate::net::{HttpClient, HttpRequest};

use super::manifest::Requirement;

/// Manifestte tanınan platform anahtarları.
///
/// Liste kapalı: yazım hatası (`linux-amd64`) sessizce "bu platform için
/// yayın yok"a dönüşmesin diye manifest yüklenirken reddediliyor. Yeni bir
/// platform eklemek `api`'yi artırmaz (§2.1: eklemek kırmaz).
pub const PLATFORMS: &[&str] = &[
    "linux-x86_64",
    "linux-aarch64",
    "linux-x86",
    "linux-arm",
    "linux-x86_64-musl",
    "linux-aarch64-musl",
    "macos-x86_64",
    "macos-aarch64",
    "windows-x86_64",
    "windows-aarch64",
    "windows-x86",
];

/// Çekirdeğin çalıştığı platformun anahtarı.
#[must_use]
pub fn current_platform() -> String {
    let libc = if cfg!(target_env = "musl") {
        "-musl"
    } else {
        ""
    };
    format!("{}-{}{libc}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Bir indirmenin kabul edeceği en büyük gövde.
///
/// Emniyet kemeri, güvenlik duvarı değil: yanlış bir adres diski
/// doldurmasın. Bugünün en büyük eseri yt-dlp'nin Linux ikilisi (~40 MB).
/// Aşıldığında **söyleniyor**, dosya yarım bırakılmıyor.
pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

/// Bir eserin diskteki durumu. **Ağa çıkmadan** ölçülür.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequirementState {
    /// Kurulu ve karması beyanla uyuşuyor.
    Installed { path: PathBuf },
    /// Hiç kurulmadı.
    Missing,
    /// Diskte var ama karması tutmuyor — yarım inmiş ya da değişmiş.
    Corrupt { expected: String, found: String },
    /// Manifest bu platform için yayın beyan etmiyor. `available`, beyan
    /// edilen platformlar — kullanıcı "hiç mi yok, yoksa yalnızca bende mi
    /// yok?" sorusunun cevabını görsün.
    Unsupported {
        platform: String,
        available: Vec<String>,
    },
}

impl RequirementState {
    /// Eklenti bununla çalışabilir mi.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Installed { .. })
    }

    /// Kullanıcıya gösterilecek tek satır.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Installed { path } => format!("kurulu ({})", path.display()),
            Self::Missing => "kurulu değil".to_owned(),
            Self::Corrupt { expected, found } => format!(
                "karma tutmuyor (beklenen {}…, bulunan {}…)",
                short_hash(expected),
                short_hash(found)
            ),
            Self::Unsupported {
                platform,
                available,
            } => unsupported_text(platform, available),
        }
    }
}

fn unsupported_text(platform: &str, available: &[String]) -> String {
    format!(
        "bu platform ({platform}) için yayın yok; beyan edilenler: {}. Kurulum bunu \
         düzeltmez — eklentinin manifesti bu platformu içermiyor",
        available.join(", ")
    )
}

/// Bir eserin adı ve durumu — `headshell plugin list` bunu basar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementStatus {
    pub name: String,
    pub version: String,
    /// Durumun ölçüldüğü platform.
    pub platform: String,
    pub state: RequirementState,
}

/// Bir kurulum denemesinin sonucu (K9: ne oldu, hangi adımda).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallOutcome {
    /// İndirildi, doğrulandı, yerine kondu.
    Installed { path: PathBuf },
    /// Zaten kuruluydu; ağa çıkılmadı.
    AlreadyInstalled { path: PathBuf },
    /// Kaynak "yok" dedi (404/410). Düzeltmesi eklenti yazarının işi.
    Orphaned { status: u16 },
    /// Ağa çıkılamadı ya da kaynak geçici bir hata döndü. Yarın tekrar dene.
    Unreachable { detail: String },
    /// İndi ama karması beyanla uyuşmadı. **Yerine konmadı.**
    HashMismatch { expected: String, found: String },
    /// Bu platform için yayın beyan edilmemiş; ağa çıkılmadı.
    Unsupported {
        platform: String,
        available: Vec<String>,
    },
}

impl InstallOutcome {
    /// Eklenti bundan sonra çalışabilir mi.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Installed { .. } | Self::AlreadyInstalled { .. })
    }

    /// Kullanıcıya gösterilecek tek satır.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Installed { path } => format!("kuruldu → {}", path.display()),
            Self::AlreadyInstalled { path } => format!("zaten kurulu ({})", path.display()),
            Self::Orphaned { status } => format!(
                "YETİM — kaynak {status} dedi: beyan edilen adres artık yok. \
                 Bu bir ağ sorunu değil; düzeltmesi eklenti yazarına ait, \
                 manifestin yeni bir sürümünü beklemek gerekiyor."
            ),
            Self::Unreachable { detail } => {
                format!("kurulamadı — kaynağa ulaşılamadı: {detail}")
            }
            Self::HashMismatch { expected, found } => format!(
                "KURULMADI — inen dosyanın karması beyanla uyuşmuyor \
                 (beklenen {}…, inen {}…). Dosya yerine konmadı.",
                short_hash(expected),
                short_hash(found)
            ),
            Self::Unsupported {
                platform,
                available,
            } => unsupported_text(platform, available),
        }
    }
}

/// Bir eklentinin bütün eserlerinin kurulum raporu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallReport {
    pub plugin: String,
    /// Her eser için ne olduğu — sırası manifestteki sıra.
    pub outcomes: Vec<(String, InstallOutcome)>,
}

impl InstallReport {
    /// Eklenti artık çalışabilir mi: eserlerin **hepsi** hazır mı.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.outcomes.iter().all(|(_, outcome)| outcome.is_ready())
    }
}

/// Açılmış bir indirme: durum kodu, bilinen uzunluk ve ilerlemeli okuyucu.
pub struct ArtifactResponse {
    pub status: u16,
    pub length: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

/// Eserin indirildiği yer.
///
/// [`HttpClient`]'tan ayrı bir trait, çünkü o gövdeyi **belleğe alır** (ve
/// 32 MB'ta keser): üstveri çağrıları için doğru, 40 MB'lık bir ikili için
/// değil. Bu trait gövdeyi okuyucu olarak verir; motor onu diske akıtırken
/// karmasını hesaplar. Somut uygulaması `http-client` feature'ında
/// (`UreqClient`), testlerde [`BufferedSource`].
pub trait ArtifactSource: Send + Sync {
    /// Adresi açar.
    ///
    /// # Errors
    /// Bağlantı kurulamazsa. **2xx dışı durum kodu hata değildir**:
    /// [`ArtifactResponse::status`] olarak döner, çünkü 404 ile 503 ayrı
    /// tanılardır (yetim / ulaşılamadı).
    fn open(&self, url: &str) -> Result<ArtifactResponse>;
}

/// Herhangi bir [`HttpClient`]'ı eser kaynağına çevirir — gövdeyi belleğe
/// alarak.
///
/// Kendi HTTP yığınını veren çağıranlar (mobil, testlerin sahte istemcisi)
/// için. Gövde tavanı istemcinin kendisinindir; büyük eserlerde akan bir
/// kaynak tercih edilmeli.
pub struct BufferedSource(pub Arc<dyn HttpClient>);

impl ArtifactSource for BufferedSource {
    fn open(&self, url: &str) -> Result<ArtifactResponse> {
        let request = HttpRequest::get(url);
        let response = super::block_on(self.0.send(&request))?;
        let length = u64::try_from(response.body.len()).ok();
        Ok(ArtifactResponse {
            status: response.status,
            length,
            body: Box::new(std::io::Cursor::new(response.body)),
        })
    }
}

/// Bu derlemenin eser kaynağı: diske akan indirme.
///
/// # Errors
/// `http-client` feature'ı kapalıysa — sessizce "ağ yok" demek yerine hangi
/// derleme kararının bunu yaptığını söyleyerek.
#[cfg(feature = "http-client")]
pub fn default_artifact_source() -> Result<Arc<dyn ArtifactSource>> {
    Ok(Arc::new(crate::net::UreqClient::for_downloads()))
}

/// Bu derlemenin eser kaynağı.
///
/// # Errors
/// Bu derlemede `http-client` kapalı olduğu için **her zaman** hata döner.
#[cfg(not(feature = "http-client"))]
pub fn default_artifact_source() -> Result<Arc<dyn ArtifactSource>> {
    crate::net::default_http_client().map(|http| {
        let source: Arc<dyn ArtifactSource> = Arc::new(BufferedSource(http));
        source
    })
}

/// Eserlerin yaşadığı depo: `<data_dir>/runtime`.
///
/// Durumu yok denecek kadar az: dizin ve platform. Platform ayrı tutuluyor ki
/// testler başka bir platformun davranışını makineyi değiştirmeden
/// sınayabilsin.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    runtime_dir: PathBuf,
    platform: String,
}

impl ArtifactStore {
    /// Yapılandırmadan kurar, bu makinenin platformuyla. Dizin **açılmaz**.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self::with_platform(config, &current_platform())
    }

    /// Başka bir platform adına kurar — sınama ve tanı için.
    #[must_use]
    pub fn with_platform(config: &Config, platform: &str) -> Self {
        Self {
            runtime_dir: config.runtime_dir(),
            platform: platform.to_owned(),
        }
    }

    /// Eserlerin yaşadığı dizin.
    #[must_use]
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    /// Deponun platformu.
    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }

    /// Bir eserin diskte olacağı yer.
    #[must_use]
    pub fn artifact_path(&self, requirement: &Requirement) -> PathBuf {
        self.runtime_dir.join(requirement.file_name(&self.platform))
    }

    fn unsupported(&self, requirement: &Requirement) -> (String, Vec<String>) {
        (
            self.platform.clone(),
            requirement.assets.keys().cloned().collect(),
        )
    }

    /// Bir eserin durumunu ölçer. **Ağa çıkmaz.**
    ///
    /// Dosya varsa karması hesaplanır: "var" ile "doğru" ayrı şeylerdir ve
    /// yarım inmiş bir dosya `Missing`'den daha kötü bir durumdur çünkü
    /// varlığı işin bittiğini düşündürür.
    ///
    /// # Errors
    /// Dosya var ama okunamıyorsa (izin gibi). Dosyanın **yokluğu** hata
    /// değil: [`RequirementState::Missing`].
    pub fn state_of(&self, requirement: &Requirement) -> Result<RequirementState> {
        let Some(asset) = requirement.asset_for(&self.platform) else {
            let (platform, available) = self.unsupported(requirement);
            return Ok(RequirementState::Unsupported {
                platform,
                available,
            });
        };
        let path = self.artifact_path(requirement);
        let found = match hash_file(&path) {
            Ok(found) => found,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RequirementState::Missing);
            }
            Err(err) => return Err(io_err(Stage::PluginRuntime, &path, err)),
        };
        let expected = asset.sha256.trim().to_lowercase();
        if found == expected {
            Ok(RequirementState::Installed { path })
        } else {
            Ok(RequirementState::Corrupt { expected, found })
        }
    }

    /// Bir eklentinin bütün eserlerinin durumu. Ağa çıkmaz.
    ///
    /// # Errors
    /// Bir eserin dosyası var ama okunamıyorsa.
    pub fn statuses(&self, requires: &[Requirement]) -> Result<Vec<RequirementStatus>> {
        requires
            .iter()
            .map(|requirement| {
                Ok(RequirementStatus {
                    name: requirement.name.clone(),
                    version: requirement.version.clone(),
                    platform: self.platform.clone(),
                    state: self.state_of(requirement)?,
                })
            })
            .collect()
    }

    /// Hazır eserlerin `ad → yol` haritası — `host.tools.run` bunu kullanır.
    ///
    /// Hazır olmayanlar haritaya **girmez**: eklenti eksik bir aracı
    /// çağırırsa motor "kurulu değil" der, boş bir yolu çalıştırmaya
    /// kalkmaz.
    ///
    /// # Errors
    /// Bir eserin dosyası var ama okunamıyorsa.
    pub fn ready_paths(&self, requires: &[Requirement]) -> Result<BTreeMap<String, PathBuf>> {
        let mut map = BTreeMap::new();
        for requirement in requires {
            if let RequirementState::Installed { path } = self.state_of(requirement)? {
                map.insert(requirement.name.clone(), path);
            }
        }
        Ok(map)
    }

    /// Bir eseri kurar: indirir, karmasını doğrular, yerine koyar.
    ///
    /// Zaten kuruluysa **ağa çıkmaz** — bir kurulum komutunun ikinci kez
    /// koşması ücretsiz olmalı. Bozuk bir dosya varsa yeniden indirilir.
    ///
    /// Gövde diske **akar** ve karma akarken hesaplanır; dosya önce koşuma
    /// özgü `.indiriliyor` uzantılı geçici bir ada yazılır (D-060), karma
    /// tutarsa yerine taşınır. Yarıda kesilen bir indirme geçerli bir eser
    /// gibi görünmemeli.
    ///
    /// # Errors
    /// Dizin açılamazsa ya da dosya yazılamazsa. **Ağ hatası `Err` değil**:
    /// [`InstallOutcome`] içinde döner, çünkü "ulaşılamadı" ile "yetim" ile
    /// "karma tutmadı" ayrı ayrı raporlanması gereken sonuçlardır.
    pub fn install(
        &self,
        source: &dyn ArtifactSource,
        requirement: &Requirement,
    ) -> Result<InstallOutcome> {
        let Some(asset) = requirement.asset_for(&self.platform) else {
            let (platform, available) = self.unsupported(requirement);
            return Ok(InstallOutcome::Unsupported {
                platform,
                available,
            });
        };
        let path = self.artifact_path(requirement);
        if let RequirementState::Installed { path } = self.state_of(requirement)? {
            return Ok(InstallOutcome::AlreadyInstalled { path });
        }

        let response = match source.open(&asset.url) {
            Ok(response) => response,
            Err(err) => {
                return Ok(InstallOutcome::Unreachable {
                    detail: err.chain_text().replace('\n', " "),
                });
            }
        };
        // 404/410 "yok" der ve yarın da yok olacaktır; 5xx "şu an olmadı" der.
        if response.status == 404 || response.status == 410 {
            return Ok(InstallOutcome::Orphaned {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Ok(InstallOutcome::Unreachable {
                detail: format!("kaynak {} durum kodu döndürdü", response.status),
            });
        }
        if let Some(length) = response.length
            && length > MAX_ARTIFACT_BYTES
        {
            return Ok(InstallOutcome::Unreachable {
                detail: too_big(length),
            });
        }

        std::fs::create_dir_all(&self.runtime_dir)
            .map_err(|err| io_err(Stage::PluginRuntime, &self.runtime_dir, err))?;
        let temp = self.runtime_dir.join(format!(
            "{}.{}-{}.indiriliyor",
            requirement.file_name(&self.platform),
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));

        let expected = asset.sha256.trim().to_lowercase();
        let found = match stream_to_file(response.body, &temp) {
            Ok(found) => found,
            Err(StreamError::Write(err)) => {
                let _ = std::fs::remove_file(&temp);
                return Err(io_err(Stage::PluginRuntime, &temp, err));
            }
            Err(StreamError::Read(detail)) => {
                let _ = std::fs::remove_file(&temp);
                return Ok(InstallOutcome::Unreachable { detail });
            }
        };
        if found != expected {
            let _ = std::fs::remove_file(&temp);
            return Ok(InstallOutcome::HashMismatch { expected, found });
        }

        // Bundan sonraki her hata yolunda geçici dosya siliniyor: yarıda
        // kalan bir kurulum ortalıkta dosya bırakmamalı.
        if let Err(err) = make_executable(&temp) {
            let _ = std::fs::remove_file(&temp);
            return Err(err);
        }
        if let Err(err) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(io_err(Stage::PluginRuntime, &path, err));
        }

        tracing::info!(
            eser = %requirement.name,
            surum = %requirement.version,
            platform = %self.platform,
            yol = %path.display(),
            "eser kuruldu"
        );
        Ok(InstallOutcome::Installed { path })
    }
}

fn too_big(bytes: u64) -> String {
    format!(
        "gövde {bytes} bayt, sınır {MAX_ARTIFACT_BYTES} bayt — bu bir eser değil, adres \
         yanlış olabilir"
    )
}

enum StreamError {
    /// Kaynaktan okunamadı — ağ tarafı, `Unreachable` olarak raporlanır.
    Read(String),
    /// Diske yazılamadı — bizim tarafımız, hata olarak döner.
    Write(std::io::Error),
}

/// Okuyucuyu dosyaya akıtır ve yazılanın sha256'sını döndürür.
fn stream_to_file(
    mut body: Box<dyn Read + Send>,
    temp: &Path,
) -> std::result::Result<String, StreamError> {
    let mut file = std::fs::File::create(temp).map_err(StreamError::Write)?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = match body.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => {
                return Err(StreamError::Read(format!(
                    "indirme {total} baytta kesildi: {err}"
                )));
            }
        };
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if total > MAX_ARTIFACT_BYTES {
            return Err(StreamError::Read(too_big(total)));
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        file.write_all(chunk).map_err(StreamError::Write)?;
    }
    file.flush().map_err(StreamError::Write)?;
    Ok(hex(&hasher.finalize()))
}

/// Bir dosyanın sha256'sı — belleğe tamamen almadan.
fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// İndirilen dosyaya çalıştırma biti verir (unix). Windows'ta kavram yok;
/// orada çalıştırılabilirliği `.exe` uzantısı taşıyor
/// ([`Requirement::file_name`]).
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut perms = std::fs::metadata(path)
        .map_err(|err| io_err(Stage::PluginRuntime, path, err))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|err| io_err(Stage::PluginRuntime, path, err))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// Baytların sha256'sı, küçük harf onaltılık.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // Biçimlendirme `String`'e yazarken hata döndüremez.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Karmanın ilk 12 hanesi — mesajda 64 hane okunmaz.
fn short_hash(hash: &str) -> &str {
    let end = hash.len().min(12);
    hash.get(..end).unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::Asset;

    fn temp_config(name: &str) -> crate::test_support::TestConfig {
        crate::test_support::TestConfig::new(&format!("artifact-{name}"))
    }

    const PLATFORM: &str = "linux-x86_64";

    fn requirement(sha256: &str) -> Requirement {
        let mut assets = BTreeMap::new();
        assets.insert(
            PLATFORM.to_owned(),
            Asset {
                url: "https://ornek.gecersiz/yt-dlp_linux".to_owned(),
                sha256: sha256.to_owned(),
            },
        );
        assets.insert(
            "windows-x86_64".to_owned(),
            Asset {
                url: "https://ornek.gecersiz/yt-dlp.exe".to_owned(),
                sha256: "b".repeat(64),
            },
        );
        Requirement {
            name: "yt-dlp".to_owned(),
            version: "2026.08.19".to_owned(),
            assets,
        }
    }

    fn store(config: &Config) -> ArtifactStore {
        ArtifactStore::with_platform(config, PLATFORM)
    }

    fn fake(route: &str, body: &str) -> BufferedSource {
        BufferedSource(Arc::new(
            crate::net::fake::FakeHttp::new().route(route, body),
        ))
    }

    #[test]
    fn the_current_platform_is_one_the_manifest_can_name() {
        // Geliştirme ve yayın makinelerinin hepsi listede olmalı; olmayan
        // bir platform "yayın yok" der ve bu testin işi o değil.
        let platform = current_platform();
        assert!(
            PLATFORMS.contains(&platform.as_str()),
            "bu makinenin platformu ({platform}) listede yok"
        );
    }

    #[test]
    fn sha256_matches_the_published_vector() {
        // NIST FIPS 180-2, ek B.1: "abc".
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_requirement_with_no_file_is_missing_not_an_error() {
        let config = temp_config("bos");
        let state = store(&config)
            .state_of(&requirement(&"a".repeat(64)))
            .unwrap();
        assert_eq!(state, RequirementState::Missing);
    }

    /// Bu platform için yayın yoksa **kurulum düzeltmez** — ve bunu söyler.
    #[test]
    fn a_platform_without_an_asset_is_unsupported_and_install_does_not_go_online() {
        let config = temp_config("platform");
        let store = ArtifactStore::with_platform(&config, "linux-arm");
        let requirement = requirement(&"a".repeat(64));

        match store.state_of(&requirement).unwrap() {
            RequirementState::Unsupported {
                platform,
                available,
            } => {
                assert_eq!(platform, "linux-arm");
                assert_eq!(available, vec!["linux-x86_64", "windows-x86_64"]);
            }
            other => panic!("beklenmeyen durum: {other:?}"),
        }

        let http = Arc::new(crate::net::fake::FakeHttp::new().route("yt-dlp", "x"));
        let source = BufferedSource(http.clone());
        let outcome = store.install(&source, &requirement).unwrap();
        assert!(matches!(outcome, InstallOutcome::Unsupported { .. }));
        assert!(
            outcome.describe().contains("yayın yok"),
            "{}",
            outcome.describe()
        );
        assert!(
            http.requests().is_empty(),
            "desteklenmeyen platform için ağa çıkıldı"
        );
    }

    #[test]
    fn a_file_whose_hash_disagrees_is_corrupt_not_installed() {
        let config = temp_config("bozuk");
        let store = store(&config);
        let requirement = requirement(&"a".repeat(64));
        std::fs::create_dir_all(store.runtime_dir()).unwrap();
        std::fs::write(store.artifact_path(&requirement), b"yarim inmis").unwrap();

        match store.state_of(&requirement).unwrap() {
            RequirementState::Corrupt { expected, found } => {
                assert_eq!(expected, "a".repeat(64));
                assert_eq!(found, sha256_hex(b"yarim inmis"));
            }
            other => panic!("bozuk dosya {other:?} diye raporlandı"),
        }
        let paths = store
            .ready_paths(std::slice::from_ref(&requirement))
            .unwrap();
        assert!(paths.is_empty(), "bozuk eser hazır sayıldı: {paths:?}");
    }

    #[test]
    fn two_platforms_never_share_a_file() {
        let config = temp_config("iki");
        let requirement = requirement(&"a".repeat(64));
        let linux = ArtifactStore::with_platform(&config, "linux-x86_64");
        let arm = ArtifactStore::with_platform(&config, "linux-aarch64");
        assert_ne!(
            linux.artifact_path(&requirement),
            arm.artifact_path(&requirement)
        );
    }

    /// Kurulumun mutlu yolu: akar, doğrulanır, yerine konur, çalıştırılabilir olur.
    #[test]
    fn a_verified_artifact_lands_on_disk_and_is_executable() {
        let config = temp_config("kurulum");
        let store = store(&config);
        let body = "#!/bin/sh\necho merhaba\n";
        let requirement = requirement(&sha256_hex(body.as_bytes()));

        let outcome = store
            .install(&fake("yt-dlp_linux", body), &requirement)
            .unwrap();
        let path = match outcome {
            InstallOutcome::Installed { path } => path,
            other => panic!("kurulmadı: {other:?}"),
        };
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        assert!(store.state_of(&requirement).unwrap().is_ready());
        assert_eq!(
            store
                .ready_paths(std::slice::from_ref(&requirement))
                .unwrap()
                .get("yt-dlp"),
            Some(&path)
        );

        let leftovers: Vec<_> = std::fs::read_dir(store.runtime_dir())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".indiriliyor"))
            .collect();
        assert!(leftovers.is_empty(), "geçici dosya kaldı: {leftovers:?}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "çalıştırma biti verilmemiş");
        }
    }

    /// Aynı eseri aynı anda kuran koşumlar birbirinin dosyasını çekmemeli (D-060).
    #[test]
    fn installing_the_same_artifact_concurrently_does_not_collide() {
        let config = temp_config("yaris");
        let body = "eser";
        let requirement = requirement(&sha256_hex(body.as_bytes()));

        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let requirement = requirement.clone();
                    let config = &config;
                    scope.spawn(move || {
                        store(config).install(&fake("yt-dlp_linux", body), &requirement)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        for result in &results {
            let outcome = result.as_ref().unwrap_or_else(|err| {
                panic!("paralel kurulum düştü:\n{}", err.chain_text());
            });
            assert!(outcome.is_ready(), "kurulum hazır değil: {outcome:?}");
        }
    }

    /// İkinci kurulum ağa **hiç** çıkmamalı.
    #[test]
    fn installing_twice_does_not_go_to_the_network_again() {
        let config = temp_config("ikinci");
        let store = store(&config);
        let body = "eser";
        let requirement = requirement(&sha256_hex(body.as_bytes()));

        let http = Arc::new(crate::net::fake::FakeHttp::new().route("yt-dlp_linux", body));
        let source = BufferedSource(http.clone());
        assert!(matches!(
            store.install(&source, &requirement).unwrap(),
            InstallOutcome::Installed { .. }
        ));
        assert!(matches!(
            store.install(&source, &requirement).unwrap(),
            InstallOutcome::AlreadyInstalled { .. }
        ));
        assert_eq!(
            http.requests().len(),
            1,
            "kurulu eser için yeniden istek atıldı"
        );
    }

    /// 404 **yetim**, 503 **ulaşılamadı** — ikisi bir cümleye toplanmamalı.
    #[test]
    fn a_dead_source_is_orphaned_but_a_flaky_one_is_only_unreachable() {
        let config = temp_config("yetim");
        let store = store(&config);
        let requirement = requirement(&"a".repeat(64));

        let gone = BufferedSource(Arc::new(crate::net::fake::FakeHttp::new().route_status(
            "yt-dlp_linux",
            404,
            "",
        )));
        assert_eq!(
            store.install(&gone, &requirement).unwrap(),
            InstallOutcome::Orphaned { status: 404 }
        );

        let flaky = BufferedSource(Arc::new(crate::net::fake::FakeHttp::new().route_status(
            "yt-dlp_linux",
            503,
            "",
        )));
        match store.install(&flaky, &requirement).unwrap() {
            InstallOutcome::Unreachable { detail } => assert!(detail.contains("503"), "{detail}"),
            other => panic!("geçici hata yetim sayıldı: {other:?}"),
        }
    }

    /// Karma tutmazsa dosya **yerine konmaz**.
    #[test]
    fn a_body_whose_hash_disagrees_is_never_written_to_disk() {
        let config = temp_config("karma");
        let store = store(&config);
        let requirement = requirement(&"a".repeat(64));

        match store
            .install(&fake("yt-dlp_linux", "baska bir sey"), &requirement)
            .unwrap()
        {
            InstallOutcome::HashMismatch { expected, found } => {
                assert_eq!(expected, "a".repeat(64));
                assert_eq!(found, sha256_hex(b"baska bir sey"));
            }
            other => panic!("uyumsuz karma kabul edildi: {other:?}"),
        }
        assert!(
            !store.artifact_path(&requirement).exists(),
            "doğrulanmamış dosya diske yazıldı"
        );
        let leftovers = std::fs::read_dir(store.runtime_dir())
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "geçici dosya kaldı");
    }

    #[test]
    fn short_hash_does_not_panic_on_a_short_string() {
        assert_eq!(short_hash("abc"), "abc");
        assert_eq!(short_hash(&"f".repeat(64)), "f".repeat(12));
    }
}
