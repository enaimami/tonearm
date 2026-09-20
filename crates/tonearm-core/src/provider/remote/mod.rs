//! Uzak sağlayıcılar: Subsonic ve Jellyfin (PLAN §1.3, D-019).
//!
//! Ortak olan burada, ayrışan alt modüllerde:
//!
//! | Ortak | Ayrışan |
//! |---|---|
//! | sunucu kaydı, kimlik saklama (D-021) | uç nokta adresleri |
//! | HTTP taşıma sınırı (D-020) | JSON şekli |
//! | `AudioSource::HttpStream` üretimi | kimlik doğrulama biçimi |
//!
//! **K2 hatırlatması:** buradan **geçmiş çekilmez.** Uzak sunucu bir ses
//! kaynağıdır; dinleme geçmişi kullanıcının kendi export dosyalarından ve
//! `tonearm play`'in ürettiği scrobble'lardan gelir.

pub mod jellyfin;
pub mod md5;
pub mod subsonic;

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::ProviderId;
use crate::net::HttpClient;

use super::Provider;

/// Kayıt dosyasının biçim sürümü. Şekil değişirse burası artar ve göç yazılır.
const SERVERS_FILE_VERSION: u32 = 1;

/// Hangi protokol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    Subsonic,
    Jellyfin,
}

impl ServerKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Subsonic => "subsonic",
            Self::Jellyfin => "jellyfin",
        }
    }

    /// Metinden okur (CLI argümanı, config dosyası).
    ///
    /// # Errors
    /// Tanınmayan bir değer verilirse — sessizce Subsonic varsaymak,
    /// kullanıcının yazım hatasını "sunucu cevap vermiyor" hatasına çevirir.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "subsonic" | "opensubsonic" | "navidrome" | "airsonic" => Ok(Self::Subsonic),
            "jellyfin" | "emby" => Ok(Self::Jellyfin),
            other => Err(Error::new(
                Stage::ConfigLoad,
                ErrorKind::InvalidInput {
                    detail: format!("bilinmeyen sunucu türü: {other:?} (subsonic | jellyfin)"),
                },
            )),
        }
    }
}

impl std::fmt::Display for ServerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Diske yazılan kimlik bilgisi.
///
/// **Parola hiçbir varyantta düz durmaz** (D-021): Subsonic'te protokolün
/// kendi salt/token yolu, Jellyfin'de bir erişim anahtarı saklanır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoredAuth {
    /// `t=md5(parola+salt)&s=salt` — Subsonic 1.13+ kimlik yolu.
    SubsonicToken { salt: String, token: String },
    /// Jellyfin erişim anahtarı (API key ya da oturum token'ı).
    ApiKey { key: String },
}

/// Kayıtlı bir uzak sunucu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteServer {
    /// Sağlayıcı kimliği: `tonearm provider test <bu>`.
    pub id: ProviderId,
    pub kind: ServerKind,
    /// Taban adres, sondaki `/` olmadan.
    pub url: String,
    pub username: String,
    pub auth: StoredAuth,
    /// Jellyfin'de `/Users/{id}/Items` için gereken kullanıcı kimliği.
    /// Kayıt anında öğrenilir; Subsonic'te `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl RemoteServer {
    /// Kullanıcıya gösterilecek ad.
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} ({})", self.url, self.kind)
    }
}

/// Yeni bir sunucu kaydı isteği (henüz kimliği çözülmemiş).
///
/// Parola **saklanmaz**; yalnızca token/anahtar türetmek için kullanılır.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewServer {
    pub id: ProviderId,
    pub kind: ServerKind,
    pub url: String,
    pub username: String,
    /// Parola. Jellyfin'de `api_key` verildiyse gereksiz.
    pub password: Option<String>,
    /// Doğrudan verilen API anahtarı (yalnızca Jellyfin).
    pub api_key: Option<String>,
    /// Kaydetmeden önce sunucuya bağlanıp kimliği doğrula.
    pub verify: bool,
}

/// Kayıt dosyasının kökü.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ServersFile {
    version: u32,
    servers: Vec<RemoteServer>,
}

/// `servers.json`'u okur. Dosya yoksa boş liste — hata değil.
///
/// # Errors
/// Dosya okunamazsa, JSON bozuksa ya da sürüm tanınmıyorsa. Bozuk dosyayı
/// **yok sayıp boş liste dönmüyoruz**: kullanıcının sunucuları sessizce
/// kaybolmuş görünürdü.
pub fn load_servers(path: &Path) -> Result<Vec<RemoteServer>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(crate::error::io_err(Stage::ConfigLoad, path, err)),
    };
    let file: ServersFile = serde_json::from_slice(&raw).map_err(|source| {
        Error::new(
            Stage::ConfigLoad,
            ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })?;
    if file.version > SERVERS_FILE_VERSION {
        return Err(Error::new(
            Stage::ConfigLoad,
            ErrorKind::InvalidInput {
                detail: format!(
                    "{} sürüm {} ile yazılmış; bu derleme en fazla {} okuyor",
                    path.display(),
                    file.version,
                    SERVERS_FILE_VERSION
                ),
            },
        ));
    }
    Ok(file.servers)
}

/// `servers.json`'u yazar. Unix'te izinler `0600`.
///
/// # Errors
/// Dizin oluşturulamaz ya da dosya yazılamazsa.
pub fn save_servers(path: &Path, servers: &[RemoteServer]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| crate::error::io_err(Stage::ConfigLoad, parent, err))?;
    }
    let file = ServersFile {
        version: SERVERS_FILE_VERSION,
        servers: servers.to_vec(),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|source| {
        Error::new(
            Stage::ConfigLoad,
            ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })?;
    std::fs::write(path, text).map_err(|err| crate::error::io_err(Stage::ConfigLoad, path, err))?;
    restrict_permissions(path)?;
    Ok(())
}

/// Dosyayı yalnızca sahibine açar (D-021).
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| crate::error::io_err(Stage::ConfigLoad, path, err))
}

/// Unix dışında izin kısıtlaması yok; sessiz kalmıyoruz, log'a düşüyor.
#[cfg(not(unix))]
fn restrict_permissions(path: &Path) -> Result<()> {
    tracing::warn!(
        path = %path.display(),
        "bu platformda dosya izni kısıtlanamıyor; kimlik bilgisi dosyasını korumak kullanıcıya kalıyor"
    );
    Ok(())
}

/// Taban adresi normalize eder: sondaki `/` gider, şema zorunlu.
///
/// # Errors
/// Adres boşsa ya da `http://` / `https://` ile başlamıyorsa. Şemayı tahmin
/// etmiyoruz: `https` varsaymak sessizce başarısız bir bağlantı, `http`
/// varsaymak sessizce şifresiz bir parola demek olurdu.
pub fn normalize_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(Error::new(
            Stage::ConfigLoad,
            ErrorKind::InvalidInput {
                detail: "sunucu adresi boş".to_owned(),
            },
        ));
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(Error::new(
            Stage::ConfigLoad,
            ErrorKind::InvalidInput {
                detail: format!(
                    "{trimmed:?} şema taşımıyor — `http://` ya da `https://` ile başlamalı"
                ),
            },
        ));
    }
    Ok(trimmed.to_owned())
}

/// Adresten bir sağlayıcı adı önerir: `https://muzik.ev:4533` → `muzik`.
///
/// Çekirdekte, CLI'de değil: bu bir veri dönüşümü ve GUI de aynı öneriyi
/// gösterecek (Altın Kural). Ad çıkarılamazsa protokolün adına düşer.
#[must_use]
pub fn suggest_id(url: &str, kind: ServerKind) -> ProviderId {
    let after_scheme = url.rsplit("://").next().unwrap_or(url);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    // Port'u at. IPv6 köşeli parantezli adreslerde bu ayrım bozulur ama
    // sonuç yalnızca bir **öneri**; kullanıcı `--name` ile ezebiliyor.
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);

    let label = host.trim_start_matches("www.").split('.').next();
    match label {
        // Sayısal etiket (IP adresi) ad olmaz: `192` diye bir sağlayıcı
        // kullanıcıya hiçbir şey anlatmaz.
        Some(label) if !label.is_empty() && label.chars().any(|c| c.is_ascii_alphabetic()) => {
            ProviderId::new(label.to_ascii_lowercase())
        }
        _ => ProviderId::new(kind.as_str()),
    }
}

/// Rastgele salt üretir.
///
/// Dönüşün ikinci ögesi entropinin güçlü olup olmadığı: `/dev/urandom`
/// okunamazsa saat + süreç kimliği tabanlı bir yedeğe düşüyoruz ve bunu
/// **söylüyoruz** (K9 — sessiz düşüş yok).
#[must_use]
pub fn random_salt() -> (String, bool) {
    // `read` değil `read_exact`: `/dev/urandom` sonsuz bir akış, tamamını
    // okumaya kalkmak süreci belleğe boğar.
    let mut bytes = [0u8; 12];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom")
        && std::io::Read::read_exact(&mut file, &mut bytes).is_ok()
    {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        return (hex, true);
    }
    // Yedek: saat + süreç kimliği. Zayıf ama salt'ın işi gizlilik değil,
    // aynı parolanın iki kurulumda aynı token'a düşmemesi.
    let seed = format!(
        "{}-{}",
        jiff::Timestamp::now().as_nanosecond(),
        std::process::id()
    );
    (md5::md5_hex(seed.as_bytes())[..24].to_owned(), false)
}

/// Kayıt isteğini diske yazılabilir bir sunucu kaydına çevirir.
///
/// Subsonic'te ağa çıkmadan token türetilir; Jellyfin'de parola verildiyse
/// `AuthenticateByName` ile bir erişim anahtarına çevrilir — yani orada ağ
/// **zorunludur**, çünkü saklanacak şey parola değildir (D-021).
///
/// `notes` kullanıcıya gösterilecek gözlemler (zayıf entropi gibi).
///
/// # Errors
/// Gerekli kimlik bilgisi eksikse, adres bozuksa ya da doğrulama başarısızsa.
pub async fn prepare_server(
    spec: &NewServer,
    http: Arc<dyn HttpClient>,
) -> Result<(RemoteServer, Vec<String>)> {
    let url = normalize_url(&spec.url)?;
    let mut notes = Vec::new();

    let mut server = match spec.kind {
        ServerKind::Subsonic => {
            if spec.api_key.is_some() {
                notes.push(
                    "Subsonic API anahtarı kabul etmiyor; parola üzerinden token türetiliyor"
                        .to_owned(),
                );
            }
            let password = spec.password.as_deref().ok_or_else(|| {
                Error::new(
                    Stage::ConfigLoad,
                    ErrorKind::InvalidInput {
                        detail: "Subsonic için parola gerekli".to_owned(),
                    },
                )
            })?;
            let (salt, strong) = random_salt();
            if !strong {
                notes.push("salt zayıf entropiyle üretildi (/dev/urandom okunamadı)".to_owned());
            }
            let token = md5::md5_hex(format!("{password}{salt}").as_bytes());
            RemoteServer {
                id: spec.id.clone(),
                kind: ServerKind::Subsonic,
                url,
                username: spec.username.clone(),
                auth: StoredAuth::SubsonicToken { salt, token },
                user_id: None,
            }
        }
        ServerKind::Jellyfin => match &spec.api_key {
            Some(key) => RemoteServer {
                id: spec.id.clone(),
                kind: ServerKind::Jellyfin,
                url,
                username: spec.username.clone(),
                auth: StoredAuth::ApiKey { key: key.clone() },
                user_id: None,
            },
            None => {
                let password = spec.password.as_deref().ok_or_else(|| {
                    Error::new(
                        Stage::ConfigLoad,
                        ErrorKind::InvalidInput {
                            detail: "Jellyfin için parola ya da API anahtarı gerekli".to_owned(),
                        },
                    )
                })?;
                notes.push("parola erişim anahtarına çevrildi; parola saklanmıyor".to_owned());
                jellyfin::authenticate(&url, &spec.username, password, spec.id.clone(), &*http)
                    .await?
            }
        },
    };

    if spec.verify {
        let provider = provider_for(&server, Arc::clone(&http));
        let health = provider.health().await?;
        if !health.reachable {
            // "erişilemedi" **demiyoruz**: sağlıksızlığın iki ayrı sebebi var
            // ve ikisi de buradan geçiyor — sunucuya ulaşılamamış olabilir
            // (`NETWORK_REQUEST`) ya da ulaşılıp kimlik reddedilmiş olabilir
            // (`PROVIDER_CALL`, "Wrong username or password"). Dıştaki cümle
            // birini seçerse yarı zaman yalan söyler; sebebi `detail`
            // taşıyor, biz yalnızca doğrulamanın geçmediğini söylüyoruz (K9).
            return Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::InvalidInput {
                    detail: format!(
                        "{} doğrulanamadı: {}",
                        server.url,
                        health
                            .detail
                            .unwrap_or_else(|| "sebep bildirilmedi".to_owned())
                    ),
                },
            ));
        }
        if let Some(detail) = health.detail {
            notes.push(detail);
        }
    }

    // Jellyfin'in `/Users/{id}/Items` uçları kullanıcı kimliği istiyor.
    // Anahtarla kaydedildiyse henüz bilmiyoruz; şimdi öğrenmek her aramada
    // fazladan bir istek atmaktan iyidir. Öğrenilemezse kayıt yine geçerli:
    // sağlayıcı çalışma anında tembel olarak sorar.
    if server.kind == ServerKind::Jellyfin && server.user_id.is_none() {
        match jellyfin::fetch_user_id(&server, &*http).await {
            Ok(id) => server.user_id = Some(id),
            Err(err) => notes.push(format!(
                "kullanıcı kimliği şimdi öğrenilemedi, ilk aramada denenecek: {}",
                err.chain_text().replace('\n', " ")
            )),
        }
    }

    Ok((server, notes))
}

/// Kayıttan çalışan bir sağlayıcı kurar.
#[must_use]
pub fn provider_for(server: &RemoteServer, http: Arc<dyn HttpClient>) -> Arc<dyn Provider> {
    match server.kind {
        ServerKind::Subsonic => Arc::new(subsonic::SubsonicProvider::new(server.clone(), http)),
        ServerKind::Jellyfin => Arc::new(jellyfin::JellyfinProvider::new(server.clone(), http)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_lose_their_trailing_slash_but_keep_their_scheme() {
        assert_eq!(
            normalize_url("https://muzik.ev/ ").unwrap(),
            "https://muzik.ev"
        );
        assert_eq!(
            normalize_url("http://127.0.0.1:4533").unwrap(),
            "http://127.0.0.1:4533"
        );
    }

    #[test]
    fn a_url_without_a_scheme_is_rejected_not_guessed() {
        let err = normalize_url("muzik.ev").unwrap_err();
        let text = err.chain_text();
        assert!(text.contains("http://"), "{text}");
    }

    #[test]
    fn server_kind_parsing_accepts_common_names() {
        assert_eq!(
            ServerKind::parse("Navidrome").unwrap(),
            ServerKind::Subsonic
        );
        assert_eq!(ServerKind::parse("jellyfin").unwrap(), ServerKind::Jellyfin);
        assert!(ServerKind::parse("plex").is_err());
    }

    #[test]
    fn suggested_names_come_from_the_host() {
        assert_eq!(
            suggest_id("https://muzik.ev:4533", ServerKind::Subsonic).as_str(),
            "muzik"
        );
        assert_eq!(
            suggest_id("https://www.Ornek.com/jellyfin", ServerKind::Jellyfin).as_str(),
            "ornek"
        );
        // IP adresi ad olmaz: protokolün adına düşer.
        assert_eq!(
            suggest_id("http://192.168.1.5:8096", ServerKind::Jellyfin).as_str(),
            "jellyfin"
        );
    }

    #[test]
    fn salts_differ_between_calls() {
        let (a, _) = random_salt();
        let (b, _) = random_salt();
        assert_ne!(a, b, "aynı salt iki kayıtta aynı token demek olurdu");
        assert!(a.len() >= 24, "salt çok kısa: {a}");
    }

    #[test]
    fn servers_survive_a_file_round_trip_and_stay_private() {
        let dir = std::env::temp_dir().join(format!("tonearm-servers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("servers.json");

        let servers = vec![RemoteServer {
            id: ProviderId::new("ev"),
            kind: ServerKind::Subsonic,
            url: "https://muzik.ev".to_owned(),
            username: "enai".to_owned(),
            auth: StoredAuth::SubsonicToken {
                salt: "c19b2d".to_owned(),
                token: "26719a1196d2a940705a59634eb18eab".to_owned(),
            },
            user_id: None,
        }];
        save_servers(&path, &servers).unwrap();
        assert_eq!(load_servers(&path).unwrap(), servers);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "kimlik dosyası herkese açık olmamalı");
        }

        // Parola dosyaya hiç girmemeli.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("sesame"), "{text}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_an_empty_list_but_a_broken_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("tonearm-servers-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("yok.json");
        assert!(load_servers(&missing).unwrap().is_empty());

        let broken = dir.join("bozuk.json");
        std::fs::write(&broken, "{ bu json değil").unwrap();
        assert!(
            load_servers(&broken).is_err(),
            "bozuk kayıt dosyası sessizce boş sayılmamalı"
        );

        let future = dir.join("gelecek.json");
        std::fs::write(&future, r#"{"version":99,"servers":[]}"#).unwrap();
        assert!(
            load_servers(&future).is_err(),
            "bilinmeyen sürüm okunmamalı"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
