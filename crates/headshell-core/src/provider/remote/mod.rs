//! Remote providers: Subsonic and Jellyfin (PLAN §1.3, D-019).
//!
//! What they share is here, what differs is in the submodules:
//!
//! | Shared | Different |
//! |---|---|
//! | server records, credential storage (D-021) | endpoint addresses |
//! | the HTTP transport boundary (D-020) | JSON shape |
//! | producing `AudioSource::HttpStream` | authentication scheme |
//!
//! **K2 reminder:** **no history is pulled** from here. A remote server is an
//! audio source; the listening history comes from the user's own export
//! files and the scrobbles `headshell play` produces.

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

use super::{ArtworkImage, Provider};

/// A server's answer as a cover, if it is one (D-076): an `image/*` type,
/// or bytes that read as an image. Servers put their errors in the same
/// place — a Subsonic error envelope, a Jellyfin HTML page — and those are
/// not handed on as pictures.
pub(crate) fn as_image(response: &crate::net::HttpResponse) -> Option<ArtworkImage> {
    let typed = response
        .header("content-type")
        .map(str::to_ascii_lowercase)
        .filter(|kind| kind.starts_with("image/"));
    if typed.is_none() && crate::artwork::image::probe(&response.body).is_err() {
        return None;
    }
    Some(ArtworkImage {
        bytes: response.body.clone(),
        mime: typed,
        source: crate::artwork::ArtworkSource::Provider,
    })
}

/// The format version of the record file. If the shape changes this goes up
/// and a migration is written.
const SERVERS_FILE_VERSION: u32 = 1;

/// Which protocol.
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

    /// Reads it from text (a CLI argument, a config file).
    ///
    /// # Errors
    /// If an unrecognised value is given — silently assuming Subsonic would turn
    /// the user's typo into a "server does not answer" error.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "subsonic" | "opensubsonic" | "navidrome" | "airsonic" => Ok(Self::Subsonic),
            "jellyfin" | "emby" => Ok(Self::Jellyfin),
            other => Err(Error::new(
                Stage::ConfigLoad,
                ErrorKind::InvalidInput {
                    detail: format!("unknown server type: {other:?} (subsonic | jellyfin)"),
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

/// The credentials written to disk.
///
/// **In no variant is the password stored in plain text** (D-021): for
/// Subsonic the protocol's own salt/token scheme is stored, for Jellyfin an
/// access key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoredAuth {
    /// `t=md5(password+salt)&s=salt` — the Subsonic 1.13+ authentication scheme.
    SubsonicToken { salt: String, token: String },
    /// A Jellyfin access key (an API key or a session token).
    ApiKey { key: String },
}

/// A registered remote server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteServer {
    /// The provider id: `headshell provider test <this>`.
    pub id: ProviderId,
    pub kind: ServerKind,
    /// The base address, without the trailing `/`.
    pub url: String,
    pub username: String,
    pub auth: StoredAuth,
    /// The user id needed for `/Users/{id}/Items` on Jellyfin. Learned at
    /// registration time; `None` for Subsonic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl RemoteServer {
    /// The name shown to the user.
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} ({})", self.url, self.kind)
    }
}

/// A request to register a new server (its credentials not yet resolved).
///
/// The password **is not stored**; it is only used to derive a token/key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewServer {
    pub id: ProviderId,
    pub kind: ServerKind,
    pub url: String,
    pub username: String,
    /// The password. Not needed on Jellyfin if an `api_key` was given.
    pub password: Option<String>,
    /// An API key given directly (Jellyfin only).
    pub api_key: Option<String>,
    /// Connect to the server and verify the credentials before saving.
    pub verify: bool,
}

/// The root of the record file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ServersFile {
    version: u32,
    servers: Vec<RemoteServer>,
}

/// Reads `servers.json`. If there is no file, an empty list — not an error.
///
/// # Errors
/// If the file cannot be read, the JSON is corrupt or the version is not
/// recognised. We **do not ignore a corrupt file and return an empty list**:
/// the user's servers would look as if they had silently vanished.
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
                    "{} was written with version {}; this build reads at most {}",
                    path.display(),
                    file.version,
                    SERVERS_FILE_VERSION
                ),
            },
        ));
    }
    Ok(file.servers)
}

/// Writes `servers.json`. Permissions `0600` on Unix.
///
/// # Errors
/// If the directory cannot be created or the file cannot be written.
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

/// Opens the file only to its owner (D-021).
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| crate::error::io_err(Stage::ConfigLoad, path, err))
}

/// No permission restriction outside Unix; we do not stay silent, it goes to
/// the log.
#[cfg(not(unix))]
fn restrict_permissions(path: &Path) -> Result<()> {
    tracing::warn!(
        path = %path.display(),
        "file permissions cannot be restricted on this platform; protecting the credentials file is up to the user"
    );
    Ok(())
}

/// Normalises the base address: the trailing `/` goes, a scheme is required.
///
/// # Errors
/// If the address is empty or does not start with `http://` / `https://`. We
/// do not guess the scheme: assuming `https` would mean a silently failing
/// connection, assuming `http` a silently unencrypted password.
pub fn normalize_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(Error::new(
            Stage::ConfigLoad,
            ErrorKind::InvalidInput {
                detail: "the server address is empty".to_owned(),
            },
        ));
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(Error::new(
            Stage::ConfigLoad,
            ErrorKind::InvalidInput {
                detail: format!(
                    "{trimmed:?} has no scheme — it must start with `http://` or `https://`"
                ),
            },
        ));
    }
    Ok(trimmed.to_owned())
}

/// Suggests a provider name from the address: `https://music.home:4533` →
/// `music`.
///
/// In the core, not in the CLI: this is a data transformation and the GUI
/// will show the same suggestion (the Golden Rule). If no name can be
/// extracted it falls back to the protocol's name.
#[must_use]
pub fn suggest_id(url: &str, kind: ServerKind) -> ProviderId {
    let after_scheme = url.rsplit("://").next().unwrap_or(url);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    // Drop the port. This split breaks on bracketed IPv6 addresses, but the
    // result is only a **suggestion**; the user can override it with `--name`.
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);

    let label = host.trim_start_matches("www.").split('.').next();
    match label {
        // A numeric label (an IP address) does not make a name: a provider
        // called `192` tells the user nothing.
        Some(label) if !label.is_empty() && label.chars().any(|c| c.is_ascii_alphabetic()) => {
            ProviderId::new(label.to_ascii_lowercase())
        }
        _ => ProviderId::new(kind.as_str()),
    }
}

/// Produces a random salt.
///
/// The second item of the return value is whether the entropy came from the
/// operating system.
///
/// On Unix `/dev/urandom` is read. That file does not exist on Windows, and
/// the first version always fell back to the weak fallback there — not
/// silently, but always (D-070). Now the second route is the standard
/// library's `RandomState`: it takes its keys from the operating system's
/// random number generator (`ProcessPrng` on Windows), and a counter mixed
/// with those keys cannot be predicted from outside. The salt's job is not
/// secrecy — it is keeping the same password from landing on the same token
/// in two installs — and for that this is enough. The clock + process id
/// fallback only remains if both fail, and **it is said** (K9).
#[must_use]
pub fn random_salt() -> (String, bool) {
    // `read_exact`, not `read`: `/dev/urandom` is an endless stream; trying to
    // read all of it would drown the process in memory.
    let mut bytes = [0u8; 12];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom")
        && std::io::Read::read_exact(&mut file, &mut bytes).is_ok()
    {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        return (hex, true);
    }
    if let Some(hex) = os_seeded_hex() {
        return (hex, true);
    }
    // Fallback: the clock + the process id. Weak, but the salt's job is not
    // secrecy, it is keeping the same password from landing on the same token
    // in two installs.
    let seed = format!(
        "{}-{}",
        jiff::Timestamp::now().as_nanosecond(),
        std::process::id()
    );
    (md5::md5_hex(seed.as_bytes())[..24].to_owned(), false)
}

/// 24 hex digits keyed with the operating system's randomness.
///
/// `RandomState::new()` takes its keys from the operating system once per
/// process; two different counters are mixed for two independent 64-bit
/// outputs. `Option` because we want to keep the return value honest in the
/// signature: today it is always `Some`.
fn os_seeded_hex() -> Option<String> {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    let high = state.hash_one(0x6865_6164_u64);
    let low = state.hash_one(0x7368_656c_u64);
    let mut hex = format!("{high:016x}{low:016x}");
    hex.truncate(24);
    Some(hex)
}

/// Turns a registration request into a server record that can be written to
/// disk.
///
/// For Subsonic a token is derived without going online; for Jellyfin, if a
/// password was given, it is turned into an access key with
/// `AuthenticateByName` — so there the network **is required**, because what
/// gets stored is not the password (D-021).
///
/// `notes` are observations to show the user (like weak entropy).
///
/// # Errors
/// If a required credential is missing, the address is malformed or the
/// verification fails.
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
                    "Subsonic does not accept an API key; deriving a token from the password"
                        .to_owned(),
                );
            }
            let password = spec.password.as_deref().ok_or_else(|| {
                Error::new(
                    Stage::ConfigLoad,
                    ErrorKind::InvalidInput {
                        detail: "a password is required for Subsonic".to_owned(),
                    },
                )
            })?;
            let (salt, strong) = random_salt();
            if !strong {
                notes.push(
                    "the salt was produced with weak entropy (/dev/urandom could not be read)"
                        .to_owned(),
                );
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
                            detail: "a password or an API key is required for Jellyfin".to_owned(),
                        },
                    )
                })?;
                notes.push(
                    "the password was turned into an access key; the password is not stored"
                        .to_owned(),
                );
                jellyfin::authenticate(&url, &spec.username, password, spec.id.clone(), &*http)
                    .await?
            }
        },
    };

    if spec.verify {
        let provider = provider_for(&server, Arc::clone(&http));
        let health = provider.health().await?;
        if !health.reachable {
            // We **do not say** "unreachable": being unhealthy has two separate
            // causes and both come through here — the server may not have been
            // reached (`NETWORK_REQUEST`), or it may have been reached and the
            // credentials refused (`PROVIDER_CALL`, "Wrong username or
            // password"). If the outer sentence picked one it would lie half
            // the time; `detail` carries the cause, and we only say that the
            // verification did not pass (K9).
            return Err(Error::new(
                Stage::ProviderCall,
                ErrorKind::InvalidInput {
                    detail: format!(
                        "could not verify {}: {}",
                        server.url,
                        health
                            .detail
                            .unwrap_or_else(|| "no reason given".to_owned())
                    ),
                },
            ));
        }
        if let Some(detail) = health.detail {
            notes.push(detail);
        }
    }

    // Jellyfin's `/Users/{id}/Items` endpoints want a user id. If it was
    // registered with a key we do not know it yet; learning it now is
    // better than sending an extra request with every search. If it
    // cannot be learned the record is still valid: the provider asks
    // lazily at run time.
    if server.kind == ServerKind::Jellyfin && server.user_id.is_none() {
        match jellyfin::fetch_user_id(&server, &*http).await {
            Ok(id) => server.user_id = Some(id),
            Err(err) => notes.push(format!(
                "the user id could not be learned now; it will be tried on the first search: {}",
                err.chain_text().replace('\n', " ")
            )),
        }
    }

    Ok((server, notes))
}

/// Sets up a working provider from a record.
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
            normalize_url("https://music.home/ ").unwrap(),
            "https://music.home"
        );
        assert_eq!(
            normalize_url("http://127.0.0.1:4533").unwrap(),
            "http://127.0.0.1:4533"
        );
    }

    #[test]
    fn a_url_without_a_scheme_is_rejected_not_guessed() {
        let err = normalize_url("music.home").unwrap_err();
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
            suggest_id("https://music.home:4533", ServerKind::Subsonic).as_str(),
            "music"
        );
        assert_eq!(
            suggest_id("https://www.Example.com/jellyfin", ServerKind::Jellyfin).as_str(),
            "example"
        );
        // An IP address does not make a name: it falls back to the protocol's name.
        assert_eq!(
            suggest_id("http://192.168.1.5:8096", ServerKind::Jellyfin).as_str(),
            "jellyfin"
        );
    }

    /// The path of systems without `/dev/urandom` (Windows). On Linux
    /// `random_salt` never calls it; without this test it would never run.
    #[test]
    fn os_seeded_salts_are_24_hex_digits_and_differ() {
        let a = os_seeded_hex().unwrap();
        let b = os_seeded_hex().unwrap();
        assert_eq!(a.len(), 24, "{a}");
        assert!(a.bytes().all(|byte| byte.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b, "two calls gave the same salt");
    }

    #[test]
    fn salts_differ_between_calls() {
        let (a, _) = random_salt();
        let (b, _) = random_salt();
        assert_ne!(
            a, b,
            "the same salt would mean the same token in two records"
        );
        assert!(a.len() >= 24, "the salt is too short: {a}");
    }

    #[test]
    fn servers_survive_a_file_round_trip_and_stay_private() {
        let dir = crate::test_support::TempDir::new("servers");
        let path = dir.join("servers.json");

        let servers = vec![RemoteServer {
            id: ProviderId::new("ev"),
            kind: ServerKind::Subsonic,
            url: "https://music.home".to_owned(),
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
            assert_eq!(
                mode & 0o777,
                0o600,
                "the credentials file must not be open to everyone"
            );
        }

        // The password must never get into the file.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("sesame"), "{text}");
    }

    #[test]
    fn a_missing_file_is_an_empty_list_but_a_broken_one_is_an_error() {
        let dir = crate::test_support::TempDir::new("servers-bad");
        let missing = dir.join("missing.json");
        assert!(load_servers(&missing).unwrap().is_empty());

        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{ this is not json").unwrap();
        assert!(
            load_servers(&broken).is_err(),
            "a corrupt record file must not silently count as empty"
        );

        let future = dir.join("future.json");
        std::fs::write(&future, r#"{"version":99,"servers":[]}"#).unwrap();
        assert!(
            load_servers(&future).is_err(),
            "an unknown version must not be read"
        );
    }
}
