//! The engine's artifact side: installing the tools plugins declare (D-055,
//! D-069).
//!
//! D-049 set the rule — **no plugin may ask for root or a system-wide
//! install.** D-055 implemented it as "the plugin declares a pinned artifact,
//! the engine downloads it and verifies its hash". D-069 changed two things:
//!
//! 1. **No Python.** In api 1 the artifact was yt-dlp's zipapp, which needed
//!    the system's Python to run — that was the source of "whoever I sent it
//!    to had a problem". In api 2 an artifact is declared **per platform**,
//!    and a self-contained binary is chosen for each platform (yt-dlp's
//!    PyInstaller builds carry their own Python inside).
//! 2. **Streaming downloads.** The self-contained binary is ~40 MB; the HTTP
//!    client's path that buffers the body in memory (32 MB cap) is not enough
//!    for it. The download now streams to disk and the hash is computed while
//!    it streams ([`ArtifactSource`]).
//!
//! ## The platform key
//!
//! `<operating system>-<architecture>[-musl]`, both parts Rust's
//! `std::env::consts` names: `linux-x86_64`, `macos-aarch64`, `windows-x86`.
//! The key comes **from the target the core was built for**; the system is not
//! probed at runtime: if the core is a `musl` build it asks for the musl
//! binary, because on that machine it is not known that a glibc binary would
//! run anyway.
//!
//! ## Four separate diagnoses, and a fifth (K9)
//!
//! An artifact "not being ready" is not one thing:
//!
//! - [`RequirementState::Missing`] — never installed. `headshell plugin install`.
//! - [`RequirementState::Corrupt`] — on disk, but its hash does not match.
//! - [`RequirementState::Unsupported`] — **no** release for this platform.
//!   Installing does not fix it; the plugin author has to add that platform to
//!   the manifest, or the artifact itself does not support that platform.
//! - **Could not install** — could not go online. Try again tomorrow.
//! - **Orphaned** — the source said 404/410. Fixing it is the plugin author's
//!   job.
//!
//! The orphaned state **is not written to disk**: a GitHub outage returns 5xx,
//! but if it were written, a single bad moment would brand a plugin
//! permanently.

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

/// The platform keys recognised in a manifest.
///
/// The list is closed: a typo (`linux-amd64`) is rejected when the manifest is
/// loaded, so it does not silently turn into "no release for this platform".
/// Adding a platform does not bump `api` (§2.1: adding does not break).
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

/// The key of the platform the core runs on.
#[must_use]
pub fn current_platform() -> String {
    let libc = if cfg!(target_env = "musl") {
        "-musl"
    } else {
        ""
    };
    format!("{}-{}{libc}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The largest body a download accepts.
///
/// A safety belt, not a firewall: a wrong address must not fill the disk.
/// Today's largest artifact is yt-dlp's Linux binary (~40 MB). When it is
/// exceeded this **is said**, and no half file is left behind.
pub const MAX_ARTIFACT_BYTES: u64 = 128 * 1024 * 1024;

/// An artifact's state on disk. Measured **without going online**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequirementState {
    /// Installed, and its hash matches the declaration.
    Installed { path: PathBuf },
    /// Never installed.
    Missing,
    /// On disk, but its hash does not match — half downloaded, or changed.
    Corrupt { expected: String, found: String },
    /// The manifest declares no release for this platform. `available` is the
    /// declared platforms — so the user sees the answer to "is there none at all,
    /// or just none for me?".
    Unsupported {
        platform: String,
        available: Vec<String>,
    },
}

impl RequirementState {
    /// Can the plugin run with this.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Installed { .. })
    }

    /// One line to show the user.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Installed { path } => format!("installed ({})", path.display()),
            Self::Missing => "not installed".to_owned(),
            Self::Corrupt { expected, found } => format!(
                "hash mismatch (expected {}…, found {}…)",
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
        "no release for this platform ({platform}); declared: {}. Installing does not \
         fix this — the plugin's manifest does not include this platform",
        available.join(", ")
    )
}

/// An artifact's name and state — `headshell plugin list` prints this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementStatus {
    pub name: String,
    pub version: String,
    /// The platform the state was measured for.
    pub platform: String,
    pub state: RequirementState,
}

/// The result of one install attempt (K9: what happened, at which step).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallOutcome {
    /// Downloaded, verified, put in place.
    Installed { path: PathBuf },
    /// Was already installed; did not go online.
    AlreadyInstalled { path: PathBuf },
    /// The source said "gone" (404/410). Fixing it is the plugin author's job.
    Orphaned { status: u16 },
    /// Could not go online, or the source returned a temporary error. Try again
    /// tomorrow.
    Unreachable { detail: String },
    /// Downloaded, but its hash did not match the declaration. **Not put in
    /// place.**
    HashMismatch { expected: String, found: String },
    /// No release is declared for this platform; did not go online.
    Unsupported {
        platform: String,
        available: Vec<String>,
    },
}

impl InstallOutcome {
    /// Can the plugin run after this.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Installed { .. } | Self::AlreadyInstalled { .. })
    }

    /// One line to show the user.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Installed { path } => format!("installed → {}", path.display()),
            Self::AlreadyInstalled { path } => format!("already installed ({})", path.display()),
            Self::Orphaned { status } => format!(
                "ORPHANED — the source said {status}: the declared address no longer exists. \
                 This is not a network problem; fixing it is up to the plugin author, \
                 and a new version of the manifest has to be waited for."
            ),
            Self::Unreachable { detail } => {
                format!("could not install — the source could not be reached: {detail}")
            }
            Self::HashMismatch { expected, found } => format!(
                "NOT INSTALLED — the downloaded file's hash does not match the declaration \
                 (expected {}…, downloaded {}…). The file was not put in place.",
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

/// The install report for all of a plugin's artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallReport {
    pub plugin: String,
    /// What happened to each artifact — in manifest order.
    pub outcomes: Vec<(String, InstallOutcome)>,
}

impl InstallReport {
    /// Can the plugin run now: are **all** artifacts ready.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.outcomes.iter().all(|(_, outcome)| outcome.is_ready())
    }
}

/// An opened download: status code, known length and a streaming reader.
pub struct ArtifactResponse {
    pub status: u16,
    pub length: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

/// Where an artifact is downloaded from.
///
/// A separate trait from [`HttpClient`] because that one **buffers** the body
/// (and cuts it at 32 MB): right for metadata calls, not for a 40 MB binary.
/// This trait hands out the body as a reader; the engine streams it to disk
/// and computes its hash as it goes. The concrete implementation is in the
/// `http-client` feature (`UreqClient`), [`BufferedSource`] in tests.
pub trait ArtifactSource: Send + Sync {
    /// Opens the address.
    ///
    /// # Errors
    /// If no connection can be made. **A non-2xx status code is not an error**:
    /// it comes back as [`ArtifactResponse::status`], because 404 and 503 are
    /// different diagnoses (orphaned / unreachable).
    fn open(&self, url: &str) -> Result<ArtifactResponse>;
}

/// Turns any [`HttpClient`] into an artifact source — by buffering the body.
///
/// For callers that bring their own HTTP stack (mobile, the tests' fake
/// client). The body cap is the client's own; for large artifacts a
/// streaming source should be preferred.
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

/// This build's artifact source: a download that streams to disk.
///
/// # Errors
/// If the `http-client` feature is off — saying which build decision caused
/// it rather than silently saying "no network".
#[cfg(feature = "http-client")]
pub fn default_artifact_source() -> Result<Arc<dyn ArtifactSource>> {
    Ok(Arc::new(crate::net::UreqClient::for_downloads()))
}

/// This build's artifact source.
///
/// # Errors
/// In this build `http-client` is off, so it **always** returns an error.
#[cfg(not(feature = "http-client"))]
pub fn default_artifact_source() -> Result<Arc<dyn ArtifactSource>> {
    crate::net::default_http_client().map(|http| {
        let source: Arc<dyn ArtifactSource> = Arc::new(BufferedSource(http));
        source
    })
}

/// The store the artifacts live in: `<data_dir>/runtime`.
///
/// It has almost no state: a directory and a platform. The platform is kept
/// separately so tests can exercise another platform's behaviour without
/// changing the machine.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    runtime_dir: PathBuf,
    platform: String,
}

impl ArtifactStore {
    /// Sets it up from the configuration, with this machine's platform. The
    /// directory is **not created**.
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self::with_platform(config, &current_platform())
    }

    /// Sets it up for another platform — for testing and diagnostics.
    #[must_use]
    pub fn with_platform(config: &Config, platform: &str) -> Self {
        Self {
            runtime_dir: config.runtime_dir(),
            platform: platform.to_owned(),
        }
    }

    /// The directory the artifacts live in.
    #[must_use]
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    /// The store's platform.
    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }

    /// Where an artifact will be on disk.
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

    /// Measures an artifact's state. **Does not go online.**
    ///
    /// If the file exists its hash is computed: "exists" and "correct" are
    /// different things, and a half-downloaded file is worse than `Missing`,
    /// because its presence suggests the job is done.
    ///
    /// # Errors
    /// If the file exists but cannot be read (permissions, say). The file's
    /// **absence** is not an error: [`RequirementState::Missing`].
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

    /// The state of all of a plugin's artifacts. Does not go online.
    ///
    /// # Errors
    /// If an artifact's file exists but cannot be read.
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

    /// The `name → path` map of ready artifacts — `host.tools.run` uses it.
    ///
    /// Artifacts that are not ready **are not in** the map: if the plugin calls a
    /// missing tool the engine says "not installed", it does not try to run an
    /// empty path.
    ///
    /// # Errors
    /// If an artifact's file exists but cannot be read.
    pub fn ready_paths(&self, requires: &[Requirement]) -> Result<BTreeMap<String, PathBuf>> {
        let mut map = BTreeMap::new();
        for requirement in requires {
            if let RequirementState::Installed { path } = self.state_of(requirement)? {
                map.insert(requirement.name.clone(), path);
            }
        }
        Ok(map)
    }

    /// Installs an artifact: downloads it, verifies its hash, puts it in place.
    ///
    /// If it is already installed it **does not go online** — running an install
    /// command a second time should be free. If a corrupt file is there, it is
    /// downloaded again.
    ///
    /// The body **streams** to disk and the hash is computed as it streams; the
    /// file is first written under a run-specific temporary name with a
    /// `.downloading` extension (D-060), and moved into place if the hash
    /// matches. An interrupted download must not look like a valid artifact.
    ///
    /// # Errors
    /// If the directory cannot be created or the file cannot be written. **A
    /// network error is not an `Err`**: it comes back in [`InstallOutcome`],
    /// because "unreachable", "orphaned" and "hash mismatch" are outcomes that
    /// must each be reported separately.
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
        // 404/410 say "gone" and it will be gone tomorrow too; 5xx says "didn't
        // work just now".
        if response.status == 404 || response.status == 410 {
            return Ok(InstallOutcome::Orphaned {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Ok(InstallOutcome::Unreachable {
                detail: format!("the source returned status code {}", response.status),
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
        // The name is this run's own. The process id is shared by the threads
        // of one process, and the clock is no tiebreaker: macOS reads it in
        // microseconds, and two threads read the same one — the parallel
        // install test lost a file to the other thread's rename that way on
        // macOS CI. A counter settles it.
        static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = self.runtime_dir.join(format!(
            "{}.{}-{}-{run}.downloading",
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

        // Every error path from here on deletes the temporary file: an
        // interrupted install must not leave files lying around.
        if let Err(err) = make_executable(&temp) {
            let _ = std::fs::remove_file(&temp);
            return Err(err);
        }
        if let Err(err) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            return Err(io_err(Stage::PluginRuntime, &path, err));
        }

        tracing::info!(
            artifact = %requirement.name,
            version = %requirement.version,
            platform = %self.platform,
            path = %path.display(),
            "artifact installed"
        );
        Ok(InstallOutcome::Installed { path })
    }
}

fn too_big(bytes: u64) -> String {
    format!(
        "the body is {bytes} bytes, the limit is {MAX_ARTIFACT_BYTES} bytes — this is not an artifact, the \
         address may be wrong"
    )
}

enum StreamError {
    /// Could not read from the source — the network side, reported as
    /// `Unreachable`.
    Read(String),
    /// Could not write to disk — our side, returned as an error.
    Write(std::io::Error),
}

/// Streams the reader into the file and returns the sha256 of what was
/// written.
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
                    "the download broke off at {total} bytes: {err}"
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

/// A file's sha256 — without loading it all into memory. The catalog catches
/// local changes with this too (D-071).
pub(crate) fn hash_file(path: &Path) -> std::io::Result<String> {
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

/// Gives the downloaded file the execute bit (unix). Windows has no such
/// concept; there the `.exe` extension carries executability
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

/// The sha256 of some bytes, lower-case hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // Formatting into a `String` cannot return an error.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The first 12 digits of a hash — 64 digits are unreadable in a message.
pub(crate) fn short_hash(hash: &str) -> &str {
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
                url: "https://example.invalid/yt-dlp_linux".to_owned(),
                sha256: sha256.to_owned(),
            },
        );
        assets.insert(
            "windows-x86_64".to_owned(),
            Asset {
                url: "https://example.invalid/yt-dlp.exe".to_owned(),
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
        // Every development and release machine must be in the list; a platform
        // that is missing says "no release", and that is not what this test is
        // about.
        let platform = current_platform();
        assert!(
            PLATFORMS.contains(&platform.as_str()),
            "this machine's platform ({platform}) is not in the list"
        );
    }

    #[test]
    fn sha256_matches_the_published_vector() {
        // NIST FIPS 180-2, appendix B.1: "abc".
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
        let config = temp_config("empty");
        let state = store(&config)
            .state_of(&requirement(&"a".repeat(64)))
            .unwrap();
        assert_eq!(state, RequirementState::Missing);
    }

    /// If there is no release for this platform, **installing does not fix it** —
    /// and it says so.
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
            other => panic!("unexpected state: {other:?}"),
        }

        let http = Arc::new(crate::net::fake::FakeHttp::new().route("yt-dlp", "x"));
        let source = BufferedSource(http.clone());
        let outcome = store.install(&source, &requirement).unwrap();
        assert!(matches!(outcome, InstallOutcome::Unsupported { .. }));
        assert!(
            outcome.describe().contains("no release"),
            "{}",
            outcome.describe()
        );
        assert!(
            http.requests().is_empty(),
            "went online for an unsupported platform"
        );
    }

    #[test]
    fn a_file_whose_hash_disagrees_is_corrupt_not_installed() {
        let config = temp_config("corrupt");
        let store = store(&config);
        let requirement = requirement(&"a".repeat(64));
        std::fs::create_dir_all(store.runtime_dir()).unwrap();
        std::fs::write(store.artifact_path(&requirement), b"half downloaded").unwrap();

        match store.state_of(&requirement).unwrap() {
            RequirementState::Corrupt { expected, found } => {
                assert_eq!(expected, "a".repeat(64));
                assert_eq!(found, sha256_hex(b"half downloaded"));
            }
            other => panic!("a corrupt file was reported as {other:?}"),
        }
        let paths = store
            .ready_paths(std::slice::from_ref(&requirement))
            .unwrap();
        assert!(
            paths.is_empty(),
            "a corrupt artifact counted as ready: {paths:?}"
        );
    }

    #[test]
    fn two_platforms_never_share_a_file() {
        let config = temp_config("two");
        let requirement = requirement(&"a".repeat(64));
        let linux = ArtifactStore::with_platform(&config, "linux-x86_64");
        let arm = ArtifactStore::with_platform(&config, "linux-aarch64");
        assert_ne!(
            linux.artifact_path(&requirement),
            arm.artifact_path(&requirement)
        );
    }

    /// The install's happy path: streams, is verified, put in place, made
    /// executable.
    #[test]
    fn a_verified_artifact_lands_on_disk_and_is_executable() {
        let config = temp_config("install");
        let store = store(&config);
        let body = "#!/bin/sh\necho hello\n";
        let requirement = requirement(&sha256_hex(body.as_bytes()));

        let outcome = store
            .install(&fake("yt-dlp_linux", body), &requirement)
            .unwrap();
        let path = match outcome {
            InstallOutcome::Installed { path } => path,
            other => panic!("not installed: {other:?}"),
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
            .filter(|name| name.ends_with(".downloading"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "a temporary file was left behind: {leftovers:?}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "the execute bit was not set");
        }
    }

    /// Runs installing the same artifact at the same time must not pull each
    /// other's file away (D-060).
    #[test]
    fn installing_the_same_artifact_concurrently_does_not_collide() {
        let config = temp_config("race");
        let body = "artifact";
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
                panic!("a parallel install failed:\n{}", err.chain_text());
            });
            assert!(outcome.is_ready(), "the install is not ready: {outcome:?}");
        }
    }

    /// The second install must **not** go online at all.
    #[test]
    fn installing_twice_does_not_go_to_the_network_again() {
        let config = temp_config("second");
        let store = store(&config);
        let body = "artifact";
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
            "a request was sent again for an installed artifact"
        );
    }

    /// 404 is **orphaned**, 503 is **unreachable** — the two must not be merged
    /// into one sentence.
    #[test]
    fn a_dead_source_is_orphaned_but_a_flaky_one_is_only_unreachable() {
        let config = temp_config("orphaned");
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
            other => panic!("a temporary error counted as orphaned: {other:?}"),
        }
    }

    /// If the hash does not match, the file **is not put in place**.
    #[test]
    fn a_body_whose_hash_disagrees_is_never_written_to_disk() {
        let config = temp_config("hash-mismatch");
        let store = store(&config);
        let requirement = requirement(&"a".repeat(64));

        match store
            .install(&fake("yt-dlp_linux", "something else"), &requirement)
            .unwrap()
        {
            InstallOutcome::HashMismatch { expected, found } => {
                assert_eq!(expected, "a".repeat(64));
                assert_eq!(found, sha256_hex(b"something else"));
            }
            other => panic!("a mismatched hash was accepted: {other:?}"),
        }
        assert!(
            !store.artifact_path(&requirement).exists(),
            "an unverified file was written to disk"
        );
        let leftovers = std::fs::read_dir(store.runtime_dir())
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "a temporary file was left behind");
    }

    #[test]
    fn short_hash_does_not_panic_on_a_short_string() {
        assert_eq!(short_hash("abc"), "abc");
        assert_eq!(short_hash(&"f".repeat(64)), "f".repeat(12));
    }
}
