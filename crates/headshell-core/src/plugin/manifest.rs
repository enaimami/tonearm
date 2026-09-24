//! Eklenti manifesti (`plugin.json`, api 2) ve izin beyanı (D-040, D-069).
//!
//! Bir eklenti, içinde `plugin.json` olan bir dizindir; **dizin adı
//! kimliktir** (tema sisteminin kuralı, D-037). Manifestteki `name` dizin
//! adıyla uyuşmak zorunda: uyuşmazsa reddedilir, sessizce dizin adı
//! kullanılmaz — hangi adın kazandığı tahmin edilmemeli (K9).
//!
//! ## api 1 → api 2
//!
//! api 1'de manifest bir **komut** gösteriyordu (`exec`), eklenti alt süreçti
//! ve izin beyanı yalnızca bir sözleşmeydi. api 2'de manifest bir **betik**
//! gösterir (`main`), betik çekirdeğin içindeki QuickJS'te koşar ve ağ izni
//! **zorlanır**: eklenti dış dünyaya yalnızca motorun verdiği kapılardan
//! çıkabilir ve motor her kapıda beyana bakar (D-069).
//!
//! api 1'in iki alanı api 2'de anlamını yitirdi ve **sessizce yok
//! sayılmıyor**: `exec` ve `permissions.fs` görülürse manifest reddedilir.
//! Yok sayılsalardı eski bir eklenti "yüklendi" görünüp ilk çağrıda
//! anlaşılmaz biçimde düşerdi.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

use super::artifact::PLATFORMS;
use super::protocol::PLUGIN_API;

/// Manifest dosyasının adı.
pub const MANIFEST_FILE: &str = "plugin.json";

/// Bir eklentinin beyan ettiği izinler.
///
/// Boş küme "hiçbir yere çıkmıyorum" demektir ve geçerlidir.
///
/// Dosya izni **yok**: api 2'de eklenti dosya sistemine hiç dokunamaz.
/// Kalıcı bir şey saklaması gerekiyorsa motorun verdiği `host.storage`'ı
/// kullanır, ve o depo zaten yalnızca onundur.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    /// Bağlanılacak ana bilgisayarlar: `api.soundcloud.com` ya da
    /// `*.googlevideo.com`.
    ///
    /// Joker yalnızca en solda ve yalnızca **alt alan adları** için:
    /// `*.sndcdn.com`, `cf-media.sndcdn.com`'u kapsar ama `sndcdn.com`'un
    /// kendisini kapsamaz. Çıplak `*` ve `*.com` gibi tek etiketli joker
    /// reddedilir — "her yere çıkarım" diyen bir eklenti bunu tek tek
    /// yazmalı ya da kullanıcı onu reddetmeli (D-040).
    #[serde(default)]
    pub net: Vec<String>,
}

impl Permissions {
    /// Hiçbir izin istemiyor mu.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.net.is_empty()
    }

    /// Küçük harfe indirilmiş, sıralanmış ve tekilleştirilmiş kopya.
    /// Karşılaştırma bunun üstünden yapılır ki manifestteki sıra ya da harf
    /// büyüklüğü değişince yeniden onay istenmesin.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut net: Vec<String> = self
            .net
            .iter()
            .map(|item| normalize_host(item))
            .filter(|item| !item.is_empty())
            .collect();
        net.sort();
        net.dedup();
        Self { net }
    }

    /// Bu küme `granted`'ın içinde mi kalıyor?
    ///
    /// Onay büyümeyi yakalamak için var: eklenti izinlerini **küçültürse**
    /// yeniden sorulmaz, büyütürse sorulur (D-040). Joker hesaba katılır:
    /// onaylanmış `*.x.com`, sonradan istenen `a.x.com`'u kapsar.
    #[must_use]
    pub fn is_covered_by(&self, granted: &Self) -> bool {
        self.beyond(granted).is_empty()
    }

    /// `granted`'ın kapsamadığı istekler — kullanıcıya "bunlar yeni" diye
    /// gösterilecek olan liste.
    #[must_use]
    pub fn beyond(&self, granted: &Self) -> Self {
        let granted = granted.normalized();
        Self {
            net: self
                .normalized()
                .net
                .into_iter()
                .filter(|wanted| {
                    !granted
                        .net
                        .iter()
                        .any(|pattern| pattern_covers(pattern, wanted))
                })
                .collect(),
        }
    }

    /// Bir ana bilgisayara bağlanmaya izin var mı.
    #[must_use]
    pub fn allows_host(&self, host: &str) -> bool {
        let host = normalize_host(host);
        !host.is_empty()
            && self
                .net
                .iter()
                .any(|pattern| host_matches(&normalize_host(pattern), &host))
    }

    /// Bir adrese gitmeye izin var mı; yoksa **neden** olmadığı.
    ///
    /// Motor bunu eklentinin her isteğinde, izlediği her yönlendirmede ve
    /// eklentinin döndürdüğü akış adresinde sorar (D-069).
    ///
    /// # Errors
    /// Adres ayrıştırılamazsa, `http`/`https` değilse ya da ana bilgisayarı
    /// beyan edilmemişse — mesaj kullanıcıya olduğu gibi gösterilir.
    pub fn check_url(&self, url: &str) -> std::result::Result<(), String> {
        let host = url_host(url)?;
        if self.allows_host(&host) {
            Ok(())
        } else {
            Err(format!(
                "izin yok: `{host}` eklentinin beyan ettiği ağ izinleri arasında değil \
                 (permissions.net: {})",
                if self.net.is_empty() {
                    "boş".to_owned()
                } else {
                    self.normalized().net.join(", ")
                }
            ))
        }
    }

    /// Her girdinin biçimi geçerli mi.
    fn validate(&self) -> std::result::Result<(), String> {
        for entry in &self.net {
            validate_host_pattern(entry)?;
        }
        Ok(())
    }

    /// İnsan okunur özet. `headshell plugin list` ve `headshell diag` bunu basar.
    #[must_use]
    pub fn describe(&self) -> String {
        let normalized = self.normalized();
        if normalized.is_empty() {
            return "ağa çıkmıyor".to_owned();
        }
        format!("ağ: {}", normalized.net.join(", "))
    }
}

/// `Api.Example.COM.` → `api.example.com`.
fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Normalize edilmiş bir desen normalize edilmiş bir ana bilgisayarla eşleşiyor mu.
fn host_matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        // Alt alan adı şart: `*.x.com` `x.com`'u kapsamıyor. Sonek
        // karşılaştırması noktayla birlikte yapılıyor ki `kotux.com`
        // `*.x.com`'a uymasın.
        Some(base) => host.len() > base.len() + 1 && host.ends_with(&format!(".{base}")),
        None => host == pattern,
    }
}

/// Onaylanmış `granted` deseni istenen `wanted` desenini kapsıyor mu.
fn pattern_covers(granted: &str, wanted: &str) -> bool {
    if granted == wanted {
        return true;
    }
    match (granted.strip_prefix("*."), wanted.strip_prefix("*.")) {
        // `*.x.com` ⊇ `*.a.x.com`
        (Some(_), Some(inner)) => host_matches(granted, inner),
        // `*.x.com` ⊇ `a.x.com`
        (Some(_), None) => host_matches(granted, wanted),
        // Çıplak bir ad hiçbir jokeri kapsamaz.
        (None, _) => false,
    }
}

/// Bir izin girdisinin biçimini denetler.
fn validate_host_pattern(entry: &str) -> std::result::Result<(), String> {
    let normalized = normalize_host(entry);
    let (wildcard, host) = match normalized.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, normalized.as_str()),
    };
    if host.is_empty() || host.contains('*') {
        return Err(format!(
            "`permissions.net` girdisi `{entry}`: joker yalnızca en solda, `*.alan.adi` \
             biçiminde olabilir; çıplak `*` kabul edilmez"
        ));
    }
    if let Some(bad) = host
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '.' || *c == '-'))
    {
        return Err(format!(
            "`permissions.net` girdisi `{entry}`: `{bad}` bir ana bilgisayar adında olamaz — \
             şema, port ya da yol yazılmaz, yalnızca ad (`api.ornek.com`)"
        ));
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels
        .iter()
        .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
    {
        return Err(format!(
            "`permissions.net` girdisi `{entry}`: ana bilgisayar adı geçersiz"
        ));
    }
    if wildcard && labels.len() < 2 {
        return Err(format!(
            "`permissions.net` girdisi `{entry}`: tek etiketli joker bütün bir üst alan \
             adına (`.{host}`) izin vermek demek; en az `*.ornek.{host}` yazılmalı"
        ));
    }
    Ok(())
}

/// Bir adresin ana bilgisayarını çıkarır — **yalnızca** `http` ve `https`.
///
/// Bir URL crate'i eklenmedi (ağaç küçük kalmalı); yerine dar ve kuşkucu bir
/// ayrıştırıcı var. Kuşkucu olması şart: burada okunan ana bilgisayar ile
/// HTTP istemcisinin bağlandığı ana bilgisayar **aynı** olmalı. İki
/// ayrıştırıcının anlaşamadığı her biçim (ters bölü, boşluk, yüzde
/// kodlaması, IPv6 köşeli ayracı) izin kontrolünün etrafından dolanmak için
/// bir kapıdır; bu yüzden anlaşılmayan her şey **reddedilir**, tahmin
/// edilmez.
///
/// # Errors
/// Adres bu kurallara uymuyorsa, nedeniyle.
pub fn url_host(url: &str) -> std::result::Result<String, String> {
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(format!("adres anlaşılamadı (şema yok): {url}"));
    };
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return Err(format!(
            "yalnızca http ve https adreslerine gidilebilir (bulunan şema: `{scheme}`)"
        ));
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.contains('\\')
        || authority
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(format!("adresin ana bilgisayar kısmı anlaşılamadı: {url}"));
    }
    // Kullanıcı bilgisi (`kullanici@`) atılıyor; ana bilgisayar **son**
    // `@`'dan sonra başlar. `izinli.com@kotu.com` burada `kotu.com` okunur,
    // tıpkı istemcinin okuyacağı gibi.
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host_port.starts_with('[') {
        return Err(format!(
            "IPv6 adresine doğrudan gidilemez; izinler ana bilgisayar adıyla yazılır: {url}"
        ));
    }
    let host = host_port
        .split_once(':')
        .map_or(host_port, |(host, _)| host);
    let host = normalize_host(host);
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(format!("adresin ana bilgisayar kısmı anlaşılamadı: {url}"));
    }
    Ok(host)
}

/// Bir eserin bir platform için yayını: nereden inecek, karması ne.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// İndirileceği adres. `https://` şart.
    pub url: String,
    /// Beklenen sha256, onaltılık. Tutmuyorsa eser **yerine konmaz**.
    pub sha256: String,
}

/// Eklentinin motordan istediği bir eser (D-050 S2, D-055, D-069).
///
/// Eklenti **beyan eder, kurmaz.** İndirmeyi, karma doğrulamasını ve yerine
/// koymayı motor yapar; eklenti çalışırken yalnızca `host.tools.run` ile onu
/// çağırabilir. D-049'un "elleri uzun olmasın" şartı tam olarak budur.
///
/// api 2'de eser **platform başına** beyan edilir: yt-dlp Windows, macOS ve
/// Linux için ayrı ikililer yayımlıyor ve her biri kendi Python'unu içinde
/// taşıyor — kullanıcıdan hiçbir şey kurmasını istemeyen yol bu. Motor
/// çalıştığı platformun anahtarını ([`super::artifact::current_platform`])
/// haritada arar; bulamazsa eksikliği **söyler**, başka bir platformun
/// ikilisini denemez.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    /// Eserin adı (`yt-dlp`). `host.tools.run`'ın ilk argümanı bu.
    pub name: String,
    /// Sabitlenmiş sürüm. Dosya adına girer; sürüm değişince yeni bir
    /// dosya olur, eskisi yerinde durur.
    pub version: String,
    /// Platform anahtarı → yayın. Anahtarlar [`PLATFORMS`]'tan.
    pub assets: BTreeMap<String, Asset>,
}

impl Requirement {
    /// Diskteki dosyanın adı: `<ad>-<sürüm>-<platform>` (+ Windows'ta `.exe`).
    ///
    /// Sürüm ada giriyor ki iki eklenti aynı eserin iki sürümünü isteyince
    /// birbirinin dosyasını ezmesin; platform giriyor ki paylaşılan bir veri
    /// dizininde (iki makine, tek ev dizini) bir platformun ikilisi ötekinin
    /// yerine geçmesin. Uzantı Windows'ta şart: `CreateProcess` uzantısız
    /// bir dosyayı çalıştırılabilir saymaz.
    ///
    /// Uzantı **platform anahtarından** geliyor, derlendiği makineden değil:
    /// aynı eserin adı hangi makinede hesaplanırsa hesaplansın aynı çıksın.
    #[must_use]
    pub fn file_name(&self, platform: &str) -> String {
        let suffix = if platform.starts_with("windows-") {
            ".exe"
        } else {
            ""
        };
        format!(
            "{}-{}-{}{suffix}",
            sanitize(&self.name),
            sanitize(&self.version),
            sanitize(platform),
        )
    }

    /// Verilen platformun yayını.
    #[must_use]
    pub fn asset_for(&self, platform: &str) -> Option<&Asset> {
        self.assets.get(platform)
    }

    /// Beyan kendi içinde tutarlı mı.
    fn validate(&self) -> std::result::Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("`requires` girdisinin `name`'i boş".to_owned());
        }
        if self.version.trim().is_empty() {
            return Err(format!("`requires` girdisi `{}`: `version` boş", self.name));
        }
        if self.assets.is_empty() {
            return Err(format!(
                "`requires` girdisi `{}`: `assets` boş — hiçbir platform için yayın yok",
                self.name
            ));
        }
        for (platform, asset) in &self.assets {
            if !PLATFORMS.contains(&platform.as_str()) {
                return Err(format!(
                    "`requires` girdisi `{}`: `{platform}` tanınan bir platform değil. \
                     Geçerli anahtarlar: {}",
                    self.name,
                    PLATFORMS.join(", ")
                ));
            }
            // `https` şartı: karma doğrulaması indirileni sonradan denetler ama
            // düz HTTP üzerinden **hangi** adresten indirildiği de doğrulanmaz.
            if !asset.url.starts_with("https://") {
                return Err(format!(
                    "`requires` girdisi `{}` ({platform}): `url` https:// ile başlamalı \
                     (bulunan: {})",
                    self.name, asset.url
                ));
            }
            let hash = asset.sha256.trim();
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!(
                    "`requires` girdisi `{}` ({platform}): `sha256` 64 haneli onaltılık \
                     olmalı (bulunan: {} hane)",
                    self.name,
                    hash.len()
                ));
            }
        }
        Ok(())
    }
}

/// Dosya adına girecek metni zararsızlaştırır.
///
/// Manifest kullanıcının indirdiği bir dosya: içindeki bir ad `../` taşırsa
/// eser veri dizininin dışına yazılırdı.
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `plugin.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Dizin adıyla aynı olmak zorunda. Sağlayıcı kimliği bu.
    pub name: String,
    /// Kullanıcıya gösterilecek ad.
    pub display_name: String,
    /// Eklentinin kendi sürümü. Protokol sürümü değil.
    #[serde(default)]
    pub version: Option<String>,
    /// Konuştuğu protokol sürümü ([`PLUGIN_API`]).
    pub api: u32,
    /// Eklenti dizinine göre betiğin yolu (`main.js`). ES modülü olarak
    /// değerlendirilir; dışa aktardığı fonksiyonlar eklentinin yüzüdür.
    pub main: String,
    /// Yetenek adları (`search`, `stream`).
    ///
    /// **Tek kaynak bu.** api 1'de el sıkışma da yetenek bildiriyordu ve
    /// çelişkide o kazanıyordu; api 2'de el sıkışma yok. Motor eklentiyi
    /// başlatırken beyanın karşılığı olan fonksiyonların dışa aktarıldığını
    /// denetler: `stream` diyen ama `resolve_source` vermeyen bir eklenti
    /// başlamaz (K9).
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
    /// Motordan istenen eserler (D-055, D-069). Boş liste "hiçbir şey istemiyorum".
    #[serde(default)]
    pub requires: Vec<Requirement>,
    /// İsteğe bağlı bir cümlelik açıklama.
    #[serde(default)]
    pub description: Option<String>,
}

/// Manifestin, doğrulamadan önce okunabilen en küçük hâli.
///
/// Keşif sürüm uyuşmazlığını **ayrı bir tanı** olarak göstermek istiyor:
/// api 1 bir manifest `main` taşımadığı için tam ayrıştırma "`main` alanı
/// yok" diye düşerdi — doğru ama yanlış yönlendiren bir cevap. Önce sürüme
/// bakılıyor; uymuyorsa söylenecek şey "bu eklenti eski protokolü
/// konuşuyor"dur.
#[derive(Debug, Clone, Deserialize)]
struct ManifestProbe {
    #[serde(default)]
    api: Option<u32>,
}

impl PluginManifest {
    /// Bir eklenti dizininden okur ve doğrular.
    ///
    /// # Errors
    /// Dosya yoksa/okunamazsa, JSON bozuksa, protokol sürümü uymuyorsa
    /// ([`ErrorKind::PluginIncompatible`]), ad dizin adıyla uyuşmuyorsa, `main`
    /// geçersizse ya da api 1'den kalma bir alan varsa.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST_FILE);
        let raw =
            std::fs::read_to_string(&path).map_err(|err| io_err(Stage::PluginLoad, &path, err))?;
        let json_err = |source| {
            Error::new(
                Stage::PluginLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        };

        let probe: ManifestProbe = serde_json::from_str(&raw).map_err(json_err)?;
        if let Some(api) = probe.api
            && api != PLUGIN_API
        {
            return Err(Error::new(
                Stage::PluginLoad,
                ErrorKind::PluginIncompatible {
                    plugin: dir_name(dir),
                    plugin_api: api,
                    host_api: PLUGIN_API,
                },
            ));
        }

        let value: serde_json::Value = serde_json::from_str(&raw).map_err(json_err)?;
        reject_api1_leftovers(&value, &path)?;

        let manifest: Self = serde_json::from_value(value).map_err(json_err)?;
        manifest.validate(dir, &path)?;
        Ok(manifest)
    }

    fn validate(&self, dir: &Path, path: &Path) -> Result<()> {
        let invalid = |detail: String| {
            Err(Error::new(
                Stage::PluginLoad,
                ErrorKind::PluginManifest {
                    path: path.to_path_buf(),
                    detail,
                },
            ))
        };

        let dir_name = dir_name(dir);
        if self.name.trim().is_empty() {
            return invalid("`name` boş".to_owned());
        }
        if self.name != dir_name {
            return invalid(format!(
                "`name` ({}) dizin adıyla ({dir_name}) uyuşmuyor — kimlik dizin adıdır",
                self.name
            ));
        }
        if self.display_name.trim().is_empty() {
            return invalid("`display_name` boş".to_owned());
        }
        if let Err(detail) = validate_main(&self.main) {
            return invalid(detail);
        }
        if let Err(detail) = self.permissions.validate() {
            return invalid(detail);
        }
        // Beyan **yüklemede** doğrulanıyor, kurulumda değil: geçersiz bir
        // `requires` ile eklenti hiç listelenmemeli. Kurulum anına bırakılsaydı
        // kusur ancak kullanıcı komutu yazınca çıkardı.
        for requirement in &self.requires {
            if let Err(detail) = requirement.validate() {
                return invalid(detail);
            }
        }
        let mut names: Vec<&str> = self.requires.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        if names.len() != count {
            return invalid(
                "`requires` aynı adı iki kez içeriyor — hangisinin kazandığı tahmin edilmemeli"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Betiğin eklenti dizini içindeki yolu.
    ///
    /// Doğrulama `main`'in dizinin dışına çıkamayacağını zaten garanti etti;
    /// burada yalnızca birleştiriliyor.
    #[must_use]
    pub fn main_path(&self, dir: &Path) -> PathBuf {
        dir.join(&self.main)
    }
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `main` eklenti dizininde duran bir `.js` dosyası mı.
///
/// Mutlak yol ve `..` reddedilir: manifest indirilmiş bir dosya, ve betik
/// yolu dizinin dışına çıkabilseydi bir eklenti başka bir eklentinin — ya da
/// hiç eklenti olmayan bir dosyanın — kodunu çalıştırabilirdi.
fn validate_main(main: &str) -> std::result::Result<(), String> {
    if main.trim().is_empty() {
        return Err("`main` boş — çalıştırılacak bir betik yok".to_owned());
    }
    let path = Path::new(main);
    let escapes = path
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir));
    if escapes || main.contains('\\') {
        return Err(format!(
            "`main` ({main}) eklenti dizininin içinde, göreli bir yol olmalı"
        ));
    }
    if !main.ends_with(".js") {
        return Err(format!(
            "`main` ({main}) bir `.js` dosyası olmalı — eklentiler QuickJS'te koşar (D-069)"
        ));
    }
    Ok(())
}

/// api 1'den kalan ve api 2'de anlamı olmayan alanları reddeder.
///
/// `serde` tanımadığı alanı yok sayar ve bu çoğu zaman doğrudur (ileride
/// eklenen bir alan eski çekirdeği kırmamalı). Bu iki alan ise **anlam
/// taşıyordu**: yok sayılsalar, `exec`'li bir manifest yüklenmiş görünür
/// ve `fs` isteyen bir eklenti dosyaya erişebileceğini sanardı.
fn reject_api1_leftovers(value: &serde_json::Value, path: &Path) -> Result<()> {
    let invalid = |detail: &str| {
        Err(Error::new(
            Stage::PluginLoad,
            ErrorKind::PluginManifest {
                path: path.to_path_buf(),
                detail: detail.to_owned(),
            },
        ))
    };
    if value.get("exec").is_some() {
        return invalid(
            "`exec` api 2'de yok: eklenti bir komut değil bir betiktir — `\"main\": \"main.js\"` \
             yazın (D-069)",
        );
    }
    let fs = value
        .get("permissions")
        .and_then(|permissions| permissions.get("fs"))
        .and_then(serde_json::Value::as_array);
    if fs.is_some_and(|entries| !entries.is_empty()) {
        return invalid(
            "`permissions.fs` api 2'de yok: eklenti dosya sistemine erişemez; kalıcı veri \
             için `host.storage` kullanılır (D-069)",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `<geçici>/<ad>`: manifest testleri dizin adının eklenti adıyla aynı
    /// olmasını istiyor. Kök dizin değer düşünce silinir.
    struct PluginDir {
        _root: crate::test_support::TempDir,
        path: PathBuf,
    }

    impl std::ops::Deref for PluginDir {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.path
        }
    }

    fn temp_dir(name: &str) -> PluginDir {
        let root = crate::test_support::TempDir::new("plugin-manifest");
        let path = root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        PluginDir { _root: root, path }
    }

    fn write_manifest(dir: &Path, json: &str) {
        std::fs::write(dir.join(MANIFEST_FILE), json).unwrap();
    }

    fn net(hosts: &[&str]) -> Permissions {
        Permissions {
            net: hosts.iter().map(|h| (*h).to_owned()).collect(),
        }
    }

    fn asset_json() -> String {
        format!(
            r#"{{"url":"https://ornek.gecersiz/arac","sha256":"{}"}}"#,
            "a".repeat(64)
        )
    }

    #[test]
    fn a_valid_manifest_loads_with_its_permissions() {
        let dir = temp_dir("soundcloud");
        write_manifest(
            &dir,
            r#"{
                "name": "soundcloud",
                "display_name": "SoundCloud",
                "version": "0.2.0",
                "api": 2,
                "main": "main.js",
                "capabilities": ["search", "stream"],
                "permissions": {"net": ["api-v2.soundcloud.com", "*.sndcdn.com"]}
            }"#,
        );
        let manifest = PluginManifest::load(&dir).unwrap();
        assert_eq!(manifest.name, "soundcloud");
        assert_eq!(manifest.api, PLUGIN_API);
        assert_eq!(manifest.main_path(&dir), dir.join("main.js"));
        assert!(manifest.permissions.allows_host("cf-media.sndcdn.com"));
    }

    #[test]
    fn a_name_that_disagrees_with_the_directory_is_rejected_not_guessed() {
        let dir = temp_dir("soundcloud");
        write_manifest(
            &dir,
            r#"{"name":"baska","display_name":"X","api":2,"main":"main.js"}"#,
        );
        let err = PluginManifest::load(&dir).unwrap_err();
        assert_eq!(err.stage(), Stage::PluginLoad);
        assert!(
            err.chain_text().contains("dizin adıyla"),
            "{}",
            err.chain_text()
        );
    }

    /// api 1 bir manifest "`main` yok" diye değil, **sürüm uyuşmazlığı**
    /// olarak reddedilmeli — kullanıcıya söylenecek şey o.
    #[test]
    fn an_api1_manifest_is_reported_as_incompatible_not_as_broken() {
        let dir = temp_dir("eski");
        write_manifest(
            &dir,
            r#"{"name":"eski","display_name":"Eski","api":1,"exec":["python3","./main.py"]}"#,
        );
        let err = PluginManifest::load(&dir).unwrap_err();
        match err.kind() {
            ErrorKind::PluginIncompatible {
                plugin_api,
                host_api,
                ..
            } => {
                assert_eq!(*plugin_api, 1);
                assert_eq!(*host_api, PLUGIN_API);
            }
            other => panic!("beklenmeyen hata: {other:?}"),
        }
    }

    #[test]
    fn api1_fields_are_refused_not_silently_ignored() {
        for (manifest, expected) in [
            (
                r#"{"name":"p","display_name":"P","api":2,"main":"main.js","exec":["x"]}"#,
                "`exec` api 2'de yok",
            ),
            (
                r#"{"name":"p","display_name":"P","api":2,"main":"main.js",
                    "permissions":{"net":[],"fs":["/ev"]}}"#,
                "`permissions.fs` api 2'de yok",
            ),
        ] {
            let dir = temp_dir("p");
            write_manifest(&dir, manifest);
            let err = PluginManifest::load(&dir).unwrap_err();
            assert!(err.chain_text().contains(expected), "{}", err.chain_text());
        }
    }

    #[test]
    fn main_must_stay_inside_the_plugin_directory_and_be_javascript() {
        for (main, expected) in [
            ("", "`main` boş"),
            ("../baska/main.js", "göreli bir yol"),
            ("/etc/passwd.js", "göreli bir yol"),
            ("alt\\\\main.js", "göreli bir yol"),
            ("main.py", "`.js` dosyası"),
        ] {
            let dir = temp_dir("p");
            write_manifest(
                &dir,
                &format!(r#"{{"name":"p","display_name":"P","api":2,"main":"{main}"}}"#),
            );
            let err = PluginManifest::load(&dir).unwrap_err();
            assert!(
                err.chain_text().contains(expected),
                "{main}: beklenen {expected:?} yok: {}",
                err.chain_text()
            );
        }
        // Alt dizin serbest.
        let dir = temp_dir("p");
        write_manifest(
            &dir,
            r#"{"name":"p","display_name":"P","api":2,"main":"./src/main.js"}"#,
        );
        assert!(PluginManifest::load(&dir).is_ok());
    }

    #[test]
    fn a_requires_entry_must_be_pinned_verifiable_and_per_platform() {
        let good = asset_json();
        let cases = [
            (
                format!(r#"{{"name":"yt-dlp","version":"","assets":{{"linux-x86_64":{good}}}}}"#),
                "`version` boş",
            ),
            (
                r#"{"name":"yt-dlp","version":"1","assets":{}}"#.to_owned(),
                "`assets` boş",
            ),
            (
                format!(r#"{{"name":"yt-dlp","version":"1","assets":{{"linux-amd64":{good}}}}}"#),
                "tanınan bir platform değil",
            ),
            (
                r#"{"name":"yt-dlp","version":"1","assets":{"linux-x86_64":{"url":"http://a/b","sha256":"aa"}}}"#
                    .to_owned(),
                "https://",
            ),
            (
                r#"{"name":"yt-dlp","version":"1","assets":{"linux-x86_64":{"url":"https://a/b","sha256":"kisa"}}}"#
                    .to_owned(),
                "64 haneli",
            ),
        ];
        for (entry, expected) in cases {
            let dir = temp_dir("p");
            write_manifest(
                &dir,
                &format!(
                    r#"{{"name":"p","display_name":"P","api":2,"main":"main.js","requires":[{entry}]}}"#
                ),
            );
            let err = PluginManifest::load(&dir).unwrap_err();
            assert!(
                err.chain_text().contains(expected),
                "beklenen {expected:?} yok: {}",
                err.chain_text()
            );
        }
    }

    #[test]
    fn the_same_requirement_twice_is_rejected_not_silently_deduplicated() {
        let dir = temp_dir("p");
        let one = format!(
            r#"{{"name":"yt-dlp","version":"1","assets":{{"linux-x86_64":{}}}}}"#,
            asset_json()
        );
        write_manifest(
            &dir,
            &format!(
                r#"{{"name":"p","display_name":"P","api":2,"main":"main.js","requires":[{one},{one}]}}"#
            ),
        );
        let err = PluginManifest::load(&dir).unwrap_err();
        assert!(err.chain_text().contains("iki kez"), "{}", err.chain_text());
    }

    #[test]
    fn the_artifact_file_name_carries_version_and_platform_and_cannot_escape() {
        let requirement = Requirement {
            name: "../../../etc/cron.d/x".to_owned(),
            version: "1".to_owned(),
            assets: BTreeMap::new(),
        };
        let name = requirement.file_name("linux-x86_64");
        assert!(!name.contains('/'), "{name}");
        assert!(name.ends_with("-1-linux-x86_64"), "{name}");
        // Uzantı makineden değil platformdan: bu test hangi sistemde koşarsa
        // koşsun aynı iki adı görmeli.
        assert!(
            requirement
                .file_name("windows-x86_64")
                .ends_with("-1-windows-x86_64.exe"),
            "{}",
            requirement.file_name("windows-x86_64")
        );
    }

    #[test]
    fn bad_permission_entries_are_refused_with_the_reason() {
        for (entry, expected) in [
            ("*", "çıplak `*`"),
            ("*.com", "tek etiketli joker"),
            ("https://api.ornek.com", "şema, port ya da yol"),
            ("api.ornek.com:443", "şema, port ya da yol"),
            ("api.*.ornek.com", "yalnızca en solda"),
            ("-kotu.ornek.com", "geçersiz"),
        ] {
            let err = validate_host_pattern(entry).unwrap_err();
            assert!(err.contains(expected), "{entry}: {err}");
        }
        assert!(validate_host_pattern("API.Ornek.COM.").is_ok());
        assert!(validate_host_pattern("*.googlevideo.com").is_ok());
    }

    #[test]
    fn a_wildcard_covers_subdomains_but_not_the_apex_or_lookalikes() {
        let permissions = net(&["*.sndcdn.com", "soundcloud.com"]);
        assert!(permissions.allows_host("cf-media.sndcdn.com"));
        assert!(permissions.allows_host("A-V2.SNDCDN.COM."));
        assert!(!permissions.allows_host("sndcdn.com"), "apex kapsanmamalı");
        assert!(!permissions.allows_host("kotusndcdn.com"), "sonek taklidi");
        assert!(permissions.allows_host("soundcloud.com"));
        assert!(
            !permissions.allows_host("api.soundcloud.com"),
            "çıplak ad alt alanı kapsamaz"
        );
    }

    /// İzin denetimi, istemcinin bağlanacağı ana bilgisayarı okumalı —
    /// ayrıştırıcıların anlaşamadığı her biçim bir kaçış kapısı.
    #[test]
    fn url_host_reads_what_the_client_would_connect_to_and_refuses_the_rest() {
        assert_eq!(
            url_host("https://API.ornek.com/yol?q=1").unwrap(),
            "api.ornek.com"
        );
        assert_eq!(url_host("http://ornek.com:8080").unwrap(), "ornek.com");
        assert_eq!(url_host("https://ornek.com#frag").unwrap(), "ornek.com");
        assert_eq!(
            url_host("https://izinli.com@kotu.com/").unwrap(),
            "kotu.com",
            "kullanıcı bilgisi ana bilgisayar değildir"
        );
        assert_eq!(
            url_host("https://kotu.com#@izinli.com").unwrap(),
            "kotu.com",
            "parça, ana bilgisayarın parçası değildir"
        );
        for bad in [
            "ftp://ornek.com/",
            "file:///etc/passwd",
            "ornek.com/yol",
            "https://izinli.com\\@kotu.com/",
            "https://[::1]/",
            "https://ornek com/",
            "https:///yol",
            "https://%6b%6f%74%75.com/",
        ] {
            assert!(url_host(bad).is_err(), "{bad} kabul edildi");
        }
    }

    #[test]
    fn check_url_names_the_host_that_was_not_declared() {
        let permissions = net(&["api.ornek.com"]);
        assert!(permissions.check_url("https://api.ornek.com/x").is_ok());
        let err = permissions.check_url("https://kotu.com/x").unwrap_err();
        assert!(err.contains("`kotu.com`"), "{err}");
        assert!(err.contains("api.ornek.com"), "{err}");
    }

    #[test]
    fn shrinking_permissions_stays_covered_but_growing_them_does_not() {
        let granted = net(&["a.example", "b.example"]);
        assert!(net(&["a.example"]).is_covered_by(&granted));
        let bigger = net(&["a.example", "c.example"]);
        assert!(!bigger.is_covered_by(&granted));
        assert_eq!(bigger.beyond(&granted).net, vec!["c.example".to_owned()]);
    }

    #[test]
    fn a_granted_wildcard_covers_narrower_requests_but_not_the_other_way() {
        let granted = net(&["*.ornek.com"]);
        assert!(net(&["a.ornek.com"]).is_covered_by(&granted));
        assert!(net(&["*.alt.ornek.com"]).is_covered_by(&granted));
        assert!(
            !net(&["ornek.com"]).is_covered_by(&granted),
            "apex kapsanmaz"
        );

        let narrow = net(&["a.ornek.com"]);
        assert!(
            !net(&["*.ornek.com"]).is_covered_by(&narrow),
            "joker genişlemesi yeniden onay istemeli"
        );
    }

    #[test]
    fn reordering_or_recasing_permissions_does_not_ask_the_user_again() {
        let granted = net(&["b.example", "a.example"]);
        assert!(net(&["A.example", "b.example.", " "]).is_covered_by(&granted));
    }

    #[test]
    fn describe_says_what_is_asked_for_in_turkish() {
        assert_eq!(
            net(&["api.soundcloud.com"]).describe(),
            "ağ: api.soundcloud.com"
        );
        assert_eq!(Permissions::default().describe(), "ağa çıkmıyor");
    }
}
