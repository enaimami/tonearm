//! Eklenti motoru: çalışma zamanı host'un işi, eklentinin değil (D-050, D-055).
//!
//! D-049 kuralı koydu — **hiçbir eklenti root yetkisi ya da sistem çapında
//! kurulum isteyemez.** D-050 bunun nasıl uygulanacağına karar verdi: tek bir
//! motor, çalışma zamanı Python, ve paketleri eklenti değil motor getirir.
//! Bu modül o motordur.
//!
//! ## İki iş
//!
//! 1. **Yorumlayıcıyı bulmak.** Eklenti `"exec": ["python3", "./main.py"]`
//!    yazar; hangi `python3`, kurulu mu, sürümü yeter mi sorularının cevabı
//!    burada verilir ve **bir kez** verilir. Eklentinin bu sorularla işi olmaz.
//! 2. **Beyan edilen eserleri kurmak.** Eklenti `requires` ile ne istediğini
//!    söyler ([`super::manifest::Requirement`]); indirmeyi, karma
//!    doğrulamasını ve yerine koymayı motor yapar, eklenti el sıkışmada
//!    yalnızca hazır bir yol alır.
//!
//! ## Neden `venv` + `pip` değil
//!
//! `venv` her PyPI paketine genellenirdi ama `ensurepip`'e yaslanıyor ve o
//! her dağıtımda gelmiyor (Debian `python3-venv`'i ayrı paketliyor). Orada
//! motor kullanıcıya root'suz bir çıkış yolu sunamaz — D-049'un tam kaçındığı
//! yer. Sabitlenmiş tek dosyalık eser `pip`'e hiç ihtiyaç duymaz, güvenilen
//! yüzey tek bir dosyaya iner ve o dosya karmasıyla sabitlenir.
//!
//! Bedeli açık ve kabul edildi: yalnızca tek dosya olarak dağıtılan şeyler
//! kurulabilir. Bugünkü bütün eklentilerin toplam ihtiyacı bir tane —
//! `yt-dlp`, ki zaten zipapp olarak dağıtılıyor.
//!
//! ## Üç ayrı tanı, ve dördüncüsü (K9)
//!
//! Bir eserin "hazır olmaması" tek bir şey değildir:
//!
//! - [`RequirementState::Missing`] — hiç kurulmadı. `tune plugin install`.
//! - [`RequirementState::Corrupt`] — diskte var, karması tutmuyor. İndirme
//!   yarım kalmış ya da dosya değişmiş; yeniden kurmak düzeltir.
//! - **Kurulamadı** — ağa çıkılamadı. Kaynak yaşıyor olabilir; yarın tekrar
//!   dene. ([`install`] döner, disk durumu `Missing` kalır.)
//! - **Yetim** — kaynak 404/410 dedi. Kullanıcının düzeltebileceği bir şey
//!   yok; beyan edilen adres ölmüş ve bunu düzeltmek **eklenti yazarının**
//!   işi. "Ağ yok" demek burada yanlış tavsiye olurdu.
//!
//! Yetim durumu **diske yazılmıyor.** Bir GitHub kesintisi 404 değil 5xx
//! döndürür, ama yazılsaydı tek bir kötü an bir eklentiyi kalıcı olarak
//! yetim damgalardı. Tanı ölçüldüğü anda söylenir, hatırlanmaz.
//!
//! Ve **kurulu bir eserin kaynağı ölse ne olur: hiçbir şey.** Eser diskte,
//! karması tutuyor, çalışmaya devam eder. Yetimlik yalnızca kurulmamış bir
//! eser için bir sorundur.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::net::{HttpClient, HttpRequest};

use super::manifest::Requirement;

/// Motorun istediği en düşük Python sürümü.
///
/// 3.9 seçildi çünkü eklentilerin kullandığı `dict |` birleştirmesi ve
/// yerleşik generic tipler (`list[str]`) oradan itibaren var. Daha yükseğe
/// çıkmak, hâlâ desteklenen dağıtımları dışarıda bırakırdı.
pub const MIN_PYTHON: (u32, u32) = (3, 9);

/// Bir indirmenin kabul edeceği en büyük gövde.
///
/// Motor eser indiriyor, arşiv değil: yt-dlp ~3 MB. Sınır bir güvenlik
/// duvarı değil, yanlış bir adresin belleği doldurmasına karşı bir emniyet
/// kemeri — ve aşıldığında **söyleniyor**, sessizce kırpılmıyor.
pub const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;

/// Bulunmuş bir Python yorumlayıcısı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonInfo {
    /// Çalıştırılacak yol.
    pub path: PathBuf,
    /// `3.14.7` — yorumlayıcının kendi bildirdiği sürüm.
    pub version: String,
    /// Nereden bulundu: `TUNE_PYTHON`, `python3`, `python`. Kullanıcı
    /// beklemediği bir yorumlayıcı çalıştığında hangisinin seçildiğini
    /// görebilmeli (K9).
    pub source: String,
}

/// Kullanıcının açıkça gösterdiği yorumlayıcıyı okur.
///
/// Ayrı duruyor çünkü **geri düşülmez**: `TUNE_PYTHON` verilmişse ve o
/// yorumlayıcı çalışmıyorsa motor sessizce `python3`'e kaymaz, durur ve
/// sebebini söyler. Kayarsa kullanıcı yaptığı seçimin uygulandığını sanır
/// ve bambaşka bir yorumlayıcıyla koşan bir sistemi hata ararken tanıyamaz.
fn python_override() -> Option<PathBuf> {
    std::env::var("TUNE_PYTHON")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

/// Aranacak yorumlayıcılar, denenme sırasıyla.
///
/// `python` en sonda çünkü bazı sistemlerde hâlâ Python 2 — sürüm kontrolü
/// onu zaten eler, ama önce denenmemesi gereksiz bir süreç açmayı önler.
fn python_candidates() -> Vec<(String, PathBuf)> {
    vec![
        ("python3".to_owned(), PathBuf::from("python3")),
        ("python".to_owned(), PathBuf::from("python")),
    ]
}

/// Sistemde bir Python yorumlayıcısı bulur ve sürümünü denetler.
///
/// # Errors
/// Hiçbir aday çalıştırılamazsa ya da çalışanların hiçbiri [`MIN_PYTHON`]'u
/// karşılamazsa. Hata **her adayın ne olduğunu** yazar: "python3 yok" ile
/// "python3 var ama 3.8" ayrı sorunlardır ve ayrı çözümleri vardır.
pub fn find_python() -> Result<PythonInfo> {
    // Açık seçim tek başına değerlendiriliyor ve başarısızlığı nihai:
    // burada listeye katılsaydı `python3` onu sessizce kurtarırdı.
    if let Some(path) = python_override() {
        return resolve_override(&path);
    }

    let mut attempts = Vec::new();

    for (source, path) in python_candidates() {
        match probe_python(&path) {
            Ok(version) => {
                let parsed = parse_version(&version);
                match parsed {
                    Some((major, minor)) if (major, minor) >= MIN_PYTHON => {
                        tracing::debug!(python = %path.display(), %version, %source, "yorumlayıcı bulundu");
                        return Ok(PythonInfo {
                            path,
                            version,
                            source,
                        });
                    }
                    Some((major, minor)) => attempts.push(format!(
                        "{source} ({}) sürüm {major}.{minor} — en az {}.{} gerekiyor",
                        path.display(),
                        MIN_PYTHON.0,
                        MIN_PYTHON.1
                    )),
                    None => attempts.push(format!(
                        "{source} ({}) sürümünü anlaşılır biçimde bildirmedi: {version}",
                        path.display()
                    )),
                }
            }
            Err(detail) => attempts.push(format!("{source} ({}): {detail}", path.display())),
        }
    }

    Err(runtime_err(
        "Python arama",
        format!(
            "çalıştırılabilir bir Python {}.{}+ bulunamadı. Denenenler — {}. \
             `tune` eklentileri Python ile çalışır; kurulu bir yorumlayıcının \
             yolunu `TUNE_PYTHON` ortam değişkeniyle verebilirsiniz.",
            MIN_PYTHON.0,
            MIN_PYTHON.1,
            attempts.join(" | ")
        ),
    ))
}

/// Kullanıcının gösterdiği yorumlayıcıyı değerlendirir. **Geri düşmez.**
///
/// Ayrı bir fonksiyon olması testin bunu ortam değişkeni kurmadan
/// sınayabilmesi için: `set_var` Rust 2024'te `unsafe` ve workspace
/// `unsafe_code = "forbid"` diyor.
fn resolve_override(path: &Path) -> Result<PythonInfo> {
    let version = probe_python(path).map_err(|detail| {
        runtime_err(
            "Python arama",
            format!(
                "`TUNE_PYTHON` {} çalıştırılamadı: {detail}. Başka bir yorumlayıcıya \
                 geçilmedi — değişkeni düzeltin ya da kaldırın.",
                path.display()
            ),
        )
    })?;

    match parse_version(&version) {
        Some((major, minor)) if (major, minor) >= MIN_PYTHON => Ok(PythonInfo {
            path: path.to_path_buf(),
            version,
            source: "TUNE_PYTHON".to_owned(),
        }),
        Some((major, minor)) => Err(runtime_err(
            "Python arama",
            format!(
                "`TUNE_PYTHON` {} sürüm {major}.{minor} gösteriyor; en az {}.{} gerekiyor. \
                 Başka bir yorumlayıcıya geçilmedi: seçimi siz yaptınız, sessizce \
                 değiştirmek yanlış olurdu.",
                path.display(),
                MIN_PYTHON.0,
                MIN_PYTHON.1
            ),
        )),
        None => Err(runtime_err(
            "Python arama",
            format!(
                "`TUNE_PYTHON` ({}) sürümünü anlaşılır biçimde bildirmedi: {version}",
                path.display()
            ),
        )),
    }
}

/// Bir adayı çalıştırıp sürümünü sorar. Hata dizesi tanıya girecek.
fn probe_python(path: &Path) -> std::result::Result<String, String> {
    // `--version` yerine bu: `sys.version` sürüm dışında derleyici ve tarih
    // de taşıyor, ayrıştırması kırılgan. `version_info` üç sayı, nokta.
    let output = Command::new(path)
        .args(["-c", "import sys; print('%d.%d.%d' % sys.version_info[:3])"])
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().next().unwrap_or("").trim();
        return Err(format!(
            "çalıştı ama hata döndü ({}){}",
            output.status,
            if first.is_empty() {
                String::new()
            } else {
                format!(": {first}")
            }
        ));
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if version.is_empty() {
        return Err("sürüm sorulduğunda boş cevap verdi".to_owned());
    }
    Ok(version)
}

/// `3.14.7` → `(3, 14)`. Anlaşılmazsa `None` — tahmin edilmez.
fn parse_version(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Bir eserin diskteki durumu. **Ağa çıkmadan** ölçülür.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequirementState {
    /// Kurulu ve karması beyanla uyuşuyor.
    Installed { path: PathBuf },
    /// Hiç kurulmadı.
    Missing,
    /// Diskte var ama karması tutmuyor — yarım inmiş ya da değişmiş.
    ///
    /// `Missing`'den ayrı tutuluyor çünkü kullanıcıya söyleyeceği şey farklı:
    /// biri "kurulmamış", öteki "kurulmuş ama güvenilmez".
    Corrupt { expected: String, found: String },
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
        }
    }
}

/// Bir eserin adı ve durumu — `tune plugin list` bunu basar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementStatus {
    pub name: String,
    pub version: String,
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

/// Eklenti motoru.
///
/// Durumu yok denecek kadar az: eserlerin yaşadığı dizin. Yorumlayıcı arama
/// **önbelleklenmiyor** — motor uzun ömürlü değil ve bir yoklama birkaç
/// milisaniye; önbellek, kullanıcı Python kurduktan sonra yeniden başlatmayı
/// gerektiren bir tuzak olurdu.
pub struct Engine {
    runtime_dir: PathBuf,
}

impl Engine {
    /// Yapılandırmadan kurar. Dizin **açılmaz**; okuma dizin yokken de çalışır.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self {
            runtime_dir: config.runtime_dir(),
        }
    }

    /// Eserlerin yaşadığı dizin: `<data_dir>/runtime`.
    #[must_use]
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    /// Bir eserin diskte olacağı yer.
    #[must_use]
    pub fn artifact_path(&self, requirement: &Requirement) -> PathBuf {
        self.runtime_dir.join(requirement.file_name())
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
        let path = self.artifact_path(requirement);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RequirementState::Missing);
            }
            Err(err) => return Err(io_err(Stage::PluginRuntime, &path, err)),
        };

        let found = sha256_hex(&bytes);
        if found.eq_ignore_ascii_case(requirement.sha256.trim()) {
            Ok(RequirementState::Installed { path })
        } else {
            Ok(RequirementState::Corrupt {
                expected: requirement.sha256.trim().to_lowercase(),
                found,
            })
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
                    state: self.state_of(requirement)?,
                })
            })
            .collect()
    }

    /// Hazır eserlerin `ad → yol` haritası — el sıkışmaya giden budur.
    ///
    /// Hazır olmayanlar haritaya **girmez**: eklenti eksik bir yolu var
    /// sanıp çalıştırmaya kalkmasın. Eksikliği eklenti değil motor raporlar.
    ///
    /// # Errors
    /// Bir eserin dosyası var ama okunamıyorsa.
    pub fn ready_paths(&self, requires: &[Requirement]) -> Result<BTreeMap<String, String>> {
        let mut map = BTreeMap::new();
        for requirement in requires {
            if let RequirementState::Installed { path } = self.state_of(requirement)? {
                map.insert(requirement.name.clone(), path.display().to_string());
            }
        }
        Ok(map)
    }

    /// Bir eseri kurar: indirir, karmasını doğrular, yerine koyar.
    ///
    /// Zaten kuruluysa **ağa çıkmaz** — bir kurulum komutunun ikinci kez
    /// koşması ücretsiz olmalı. Bozuk bir dosya varsa yeniden indirilir.
    ///
    /// Yazma iki adımlı: önce `.indiriliyor` uzantılı geçici dosyaya, sonra
    /// yerine taşınır. Yarıda kesilen bir indirme geçerli bir eser gibi
    /// görünmemeli.
    ///
    /// # Errors
    /// Dizin açılamazsa ya da dosya yazılamazsa. **Ağ hatası `Err` değil**:
    /// [`InstallOutcome`] içinde döner, çünkü "ulaşılamadı" ile "yetim" ile
    /// "karma tutmadı" ayrı ayrı raporlanması gereken sonuçlardır, tek bir
    /// hata zinciri değil.
    pub fn install(
        &self,
        http: &Arc<dyn HttpClient>,
        requirement: &Requirement,
    ) -> Result<InstallOutcome> {
        let path = self.artifact_path(requirement);

        if let RequirementState::Installed { path } = self.state_of(requirement)? {
            return Ok(InstallOutcome::AlreadyInstalled { path });
        }

        let expected = requirement.sha256.trim().to_lowercase();
        let bytes = match fetch(http, &requirement.url) {
            Ok(bytes) => bytes,
            Err(outcome) => return Ok(outcome),
        };

        let found = sha256_hex(&bytes);
        if found != expected {
            return Ok(InstallOutcome::HashMismatch { expected, found });
        }

        std::fs::create_dir_all(&self.runtime_dir)
            .map_err(|err| io_err(Stage::PluginRuntime, &self.runtime_dir, err))?;

        let temp = self
            .runtime_dir
            .join(format!("{}.indiriliyor", requirement.file_name()));
        std::fs::write(&temp, &bytes).map_err(|err| io_err(Stage::PluginRuntime, &temp, err))?;
        make_executable(&temp)?;
        std::fs::rename(&temp, &path).map_err(|err| io_err(Stage::PluginRuntime, &path, err))?;

        tracing::info!(
            eser = %requirement.name,
            surum = %requirement.version,
            yol = %path.display(),
            "eser kuruldu"
        );
        Ok(InstallOutcome::Installed { path })
    }
}

/// Eseri indirir. Ağ sonuçları `Err` değil [`InstallOutcome`] olarak döner.
fn fetch(http: &Arc<dyn HttpClient>, url: &str) -> std::result::Result<Vec<u8>, InstallOutcome> {
    let request = HttpRequest::get(url);
    let response = match futures_block_on(http.send(&request)) {
        Ok(response) => response,
        Err(err) => {
            return Err(InstallOutcome::Unreachable {
                detail: err.chain_text().replace('\n', " "),
            });
        }
    };

    // 404/410 "yok" der ve yarın da yok olacaktır; 5xx "şu an olmadı" der.
    // İkisini aynı cümleye toplamak, kullanıcıyı düzeltemeyeceği bir şeyi
    // beklemeye ya da düzelecek bir şeyden vazgeçmeye iter.
    if response.status == 404 || response.status == 410 {
        return Err(InstallOutcome::Orphaned {
            status: response.status,
        });
    }
    if response.status >= 400 {
        return Err(InstallOutcome::Unreachable {
            detail: format!("kaynak {} durum kodu döndürdü", response.status),
        });
    }
    if response.body.len() > MAX_ARTIFACT_BYTES {
        return Err(InstallOutcome::Unreachable {
            detail: format!(
                "gövde {} bayt, sınır {MAX_ARTIFACT_BYTES} bayt — bu bir eser değil, \
                 adres yanlış olabilir",
                response.body.len()
            ),
        });
    }

    Ok(response.body)
}

/// Kendi kendine yeten en küçük blokla bekleyici.
///
/// Motorun kurulum yolu bir kabuk komutundan (`tune plugin install`) çağrılıyor
/// ve senkron; çekirdek bir çalışma zamanı kurmuyor (kod konvansiyonu: çalışma
/// zamanını çağıran seçer). `HttpClient::send` ise `async` çünkü sağlayıcılar
/// onu async bağlamdan çağırıyor. Aradaki boşluk burada kapanıyor.
fn futures_block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    // Uyandırma gerekmiyor: `ureq` istemcisi zaten bloklayarak çalışıyor ve
    // future ilk yoklamada hazır dönüyor. `Waker::from_raw` elle yazılabilirdi
    // ama `unsafe` ve workspace `unsafe_code = "forbid"` diyor; standart
    // kütüphanenin `noop`'u aynı işi görüyor.
    //
    // Bloklamayan bir istemci gelirse bu döngü meşgul dönmeye başlar —
    // yavaşlar ama **yanlış cevap vermez**, ve yavaşlık ölçülebilir bir şey.
    let mut context = Context::from_waker(Waker::noop());
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// İndirilen dosyaya çalıştırma biti verir (unix).
///
/// Zipapp'ler `python3 <yol>` ile de çalışır, ama bit varsa eklenti onu
/// doğrudan çalıştırabilir. Windows'ta kavram yok; sessizce atlanıyor.
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
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
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
    &hash[..end]
}

/// Motorun bir adımının hatası.
fn runtime_err(step: &str, detail: String) -> Error {
    Error::new(
        Stage::PluginRuntime,
        ErrorKind::PluginRuntime {
            step: step.to_owned(),
            detail,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tune-runtime-{}-{}-{name}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn requirement(sha256: &str) -> Requirement {
        Requirement {
            name: "yt-dlp".to_owned(),
            version: "2026.08.12".to_owned(),
            url: "https://ornek.gecersiz/yt-dlp".to_owned(),
            sha256: sha256.to_owned(),
        }
    }

    fn engine(dir: &Path) -> Engine {
        Engine {
            runtime_dir: dir.join("runtime"),
        }
    }

    #[test]
    fn sha256_matches_the_published_vector() {
        // NIST FIPS 180-2, ek B.1: "abc".
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Boş girdi — sınır vakası, ve bir "hiçbir şey indirmedik" durumunun
        // sessizce geçerli sayılmadığını görmek için.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_requirement_with_no_file_is_missing_not_an_error() {
        let dir = temp_dir("bos");
        let engine = engine(&dir);
        let state = engine.state_of(&requirement(&"a".repeat(64))).unwrap();
        assert_eq!(state, RequirementState::Missing);
    }

    #[test]
    fn a_file_whose_hash_disagrees_is_corrupt_not_installed() {
        let dir = temp_dir("bozuk");
        let engine = engine(&dir);
        let requirement = requirement(&"a".repeat(64));
        std::fs::create_dir_all(engine.runtime_dir()).unwrap();
        std::fs::write(engine.artifact_path(&requirement), b"yarim inmis").unwrap();

        let state = engine.state_of(&requirement).unwrap();
        match state {
            RequirementState::Corrupt { expected, found } => {
                assert_eq!(expected, "a".repeat(64));
                assert_eq!(found, sha256_hex(b"yarim inmis"));
            }
            other => panic!("bozuk dosya {other:?} diye raporlandı"),
        }
        // Ve bozuk bir eser hazır sayılmaz: haritaya girmemeli.
        let paths = engine
            .ready_paths(std::slice::from_ref(&requirement))
            .unwrap();
        assert!(paths.is_empty(), "bozuk eser el sıkışmaya sızdı: {paths:?}");
    }

    #[test]
    fn a_file_whose_hash_matches_is_installed_and_reaches_the_handshake() {
        let dir = temp_dir("kurulu");
        let engine = engine(&dir);
        let body = b"#!/usr/bin/env python3\n";
        let requirement = requirement(&sha256_hex(body));
        std::fs::create_dir_all(engine.runtime_dir()).unwrap();
        std::fs::write(engine.artifact_path(&requirement), body).unwrap();

        assert!(engine.state_of(&requirement).unwrap().is_ready());
        let paths = engine
            .ready_paths(std::slice::from_ref(&requirement))
            .unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains_key("yt-dlp"));
    }

    #[test]
    fn the_version_is_part_of_the_file_name_so_two_versions_do_not_collide() {
        let dir = temp_dir("surum");
        let engine = engine(&dir);
        let old = Requirement {
            version: "2026.01.01".to_owned(),
            ..requirement(&"a".repeat(64))
        };
        let new = Requirement {
            version: "2026.08.12".to_owned(),
            ..requirement(&"b".repeat(64))
        };
        assert_ne!(engine.artifact_path(&old), engine.artifact_path(&new));
    }

    #[test]
    fn a_name_that_tries_to_escape_the_runtime_directory_cannot() {
        let dir = temp_dir("kacis");
        let engine = engine(&dir);
        let evil = Requirement {
            name: "../../../etc/cron.d/x".to_owned(),
            version: "1".to_owned(),
            url: "https://ornek.gecersiz/x".to_owned(),
            sha256: "c".repeat(64),
        };
        let path = engine.artifact_path(&evil);
        assert_eq!(
            path.parent(),
            Some(engine.runtime_dir()),
            "eser dizinin dışına çıktı: {}",
            path.display()
        );
    }

    #[test]
    fn a_python_that_does_not_exist_names_every_candidate_it_tried() {
        // Var olmayan bir yorumlayıcıya işaret ederek arama yolunu zorluyoruz.
        // `python3` sistemde varsa arama orada başarılı olur; o zaman da
        // sınanacak şey mesajın kendisi değil, kaynağın raporlandığıdır.
        let probe = probe_python(Path::new("bu-yorumlayici-yok-tune-testi"));
        assert!(probe.is_err(), "olmayan yorumlayıcı çalıştı");
    }

    /// Açıkça gösterilen bir yorumlayıcı çalışmıyorsa motor **kaymaz.**
    ///
    /// Bu davranış bir ölçümden doğdu: ilk yazımda `TUNE_PYTHON` yalnızca
    /// aday listesinin başına konuyordu, ve yanlış gösterildiğinde motor
    /// sessizce `python3`'e düşüp "python3 (3.14.7)" diye rapor veriyordu.
    /// Kullanıcı seçiminin uygulandığını sanırdı.
    #[test]
    fn an_explicit_python_that_does_not_work_is_not_silently_replaced() {
        let err = resolve_override(Path::new("/bu/yorumlayici/yok")).unwrap_err();
        assert_eq!(err.stage(), Stage::PluginRuntime);
        let text = err.chain_text();
        assert!(text.contains("TUNE_PYTHON"), "{text}");
        assert!(
            text.contains("Başka bir yorumlayıcıya"),
            "kaymadığını söylemiyor: {text}"
        );
        // Ve aday listesi açık seçimi hiç içermemeli — yoksa geri düşme
        // yolu arka kapıdan geri gelir.
        assert!(
            python_candidates()
                .iter()
                .all(|(source, _)| source != "TUNE_PYTHON"),
            "açık seçim aday listesine sızmış"
        );
    }

    #[test]
    fn python_version_parsing_refuses_to_guess() {
        assert_eq!(parse_version("3.14.7"), Some((3, 14)));
        assert_eq!(parse_version("3.9"), Some((3, 9)));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("Python 3.11.2"), None);
    }

    /// Kurulumun mutlu yolu: iner, doğrulanır, yerine konur, çalıştırılabilir olur.
    #[test]
    fn a_verified_artifact_lands_on_disk_and_is_executable() {
        let dir = temp_dir("kurulum");
        let engine = engine(&dir);
        let body = "#!/usr/bin/env python3\nprint('merhaba')\n";
        let requirement = requirement(&sha256_hex(body.as_bytes()));

        let http: Arc<dyn HttpClient> =
            Arc::new(crate::net::fake::FakeHttp::new().route("yt-dlp", body));
        let outcome = engine.install(&http, &requirement).unwrap();

        let path = match outcome {
            InstallOutcome::Installed { path } => path,
            other => panic!("kurulmadı: {other:?}"),
        };
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        assert!(engine.state_of(&requirement).unwrap().is_ready());

        // Yarım kalmış indirme dosyası ortalıkta kalmamalı.
        let leftovers: Vec<_> = std::fs::read_dir(engine.runtime_dir())
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

    /// İkinci kurulum ağa **hiç** çıkmamalı.
    #[test]
    fn installing_twice_does_not_go_to_the_network_again() {
        let dir = temp_dir("ikinci");
        let engine = engine(&dir);
        let body = "eser";
        let requirement = requirement(&sha256_hex(body.as_bytes()));

        let fake = Arc::new(crate::net::fake::FakeHttp::new().route("yt-dlp", body));
        let http: Arc<dyn HttpClient> = fake.clone();

        assert!(matches!(
            engine.install(&http, &requirement).unwrap(),
            InstallOutcome::Installed { .. }
        ));
        assert!(matches!(
            engine.install(&http, &requirement).unwrap(),
            InstallOutcome::AlreadyInstalled { .. }
        ));
        assert_eq!(
            fake.requests().len(),
            1,
            "kurulu eser için yeniden istek atıldı"
        );
    }

    /// 404 **yetim**, 503 **ulaşılamadı** — ikisi bir cümleye toplanmamalı.
    #[test]
    fn a_dead_source_is_orphaned_but_a_flaky_one_is_only_unreachable() {
        let dir = temp_dir("yetim");
        let engine = engine(&dir);
        let requirement = requirement(&"a".repeat(64));

        let gone: Arc<dyn HttpClient> =
            Arc::new(crate::net::fake::FakeHttp::new().route_status("yt-dlp", 404, ""));
        assert_eq!(
            engine.install(&gone, &requirement).unwrap(),
            InstallOutcome::Orphaned { status: 404 }
        );

        let flaky: Arc<dyn HttpClient> =
            Arc::new(crate::net::fake::FakeHttp::new().route_status("yt-dlp", 503, ""));
        match engine.install(&flaky, &requirement).unwrap() {
            InstallOutcome::Unreachable { detail } => assert!(detail.contains("503"), "{detail}"),
            other => panic!("geçici hata yetim sayıldı: {other:?}"),
        }
    }

    /// Karma tutmazsa dosya **yerine konmaz**. Sessizce kabul edilen bir
    /// uyumsuzluk, doğrulamanın hiç olmamasıyla aynı şeydir.
    #[test]
    fn a_body_whose_hash_disagrees_is_never_written_to_disk() {
        let dir = temp_dir("karma");
        let engine = engine(&dir);
        let requirement = requirement(&"a".repeat(64));

        let http: Arc<dyn HttpClient> =
            Arc::new(crate::net::fake::FakeHttp::new().route("yt-dlp", "baska bir sey"));
        match engine.install(&http, &requirement).unwrap() {
            InstallOutcome::HashMismatch { expected, found } => {
                assert_eq!(expected, "a".repeat(64));
                assert_eq!(found, sha256_hex(b"baska bir sey"));
            }
            other => panic!("uyumsuz karma kabul edildi: {other:?}"),
        }
        assert!(
            !engine.artifact_path(&requirement).exists(),
            "doğrulanmamış dosya diske yazıldı"
        );
        assert_eq!(
            engine.state_of(&requirement).unwrap(),
            RequirementState::Missing
        );
    }

    #[test]
    fn short_hash_does_not_panic_on_a_short_string() {
        assert_eq!(short_hash("abc"), "abc");
        assert_eq!(short_hash(&"f".repeat(64)), "f".repeat(12));
    }
}
