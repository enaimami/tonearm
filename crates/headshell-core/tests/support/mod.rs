//! Shared helpers for integration tests (D-070).
//!
//! Every integration test file is a separate crate, and this module is
//! compiled into each one separately with `mod support;`. The compiler counts
//! a helper a file does not use as dead for that crate; that is why the
//! warning is off at the module level.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use headshell_core::config::Config;

/// The root tests write to: Cargo's `CARGO_TARGET_TMPDIR` (`target/tmp`).
///
/// Not the operating system's shared temporary directory. Tests delete their
/// directories, but even the leftovers of a killed run land in the project's
/// `target`, not in the machine's `/tmp`, and go away with `cargo clean`.
/// They once wrote to the shared `/tmp`, and 1.2 GB piled up on a development
/// machine — on that machine `/tmp` was a tmpfs, that is, memory.
#[must_use]
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

/// A temporary directory that deletes itself — in a failing test too (`Drop`
/// runs during a panic).
pub struct TempDir(PathBuf);

impl TempDir {
    #[must_use]
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = root();
        std::fs::create_dir_all(&base).expect("the test root must open");
        loop {
            let dir = base.join(format!(
                "{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                // A directory with the same name left over from a killed run: not used.
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!(
                    "could not open a temporary directory ({}): {err}",
                    dir.display()
                ),
            }
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.0)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "warning: could not delete the temporary directory ({}): {err}",
                self.0.display()
            );
        }
    }
}

/// A `Config` on top of a data directory that deletes itself; it can be
/// passed anywhere a `&Config` is expected.
pub struct TestConfig {
    _dir: TempDir,
    config: Config,
}

impl TestConfig {
    #[must_use]
    pub fn new(label: &str) -> Self {
        let dir = TempDir::new(label);
        let config = Config::with_data_dir(dir.path());
        Self { _dir: dir, config }
    }
}

impl std::ops::Deref for TestConfig {
    type Target = Config;

    fn deref(&self) -> &Config {
        &self.config
    }
}

/// Installs a plugin **from the live catalog** into the data directory
/// (D-071) — the same thing the user does with `headshell plugin install
/// <name>`.
///
/// Plugins do not live in this repository, they live in `headshell/plugins`;
/// the live tests take them the way the user does, and so the catalog is
/// tested end to end as well: the index, sha256, the manifest comparison,
/// the install. The address comes from `HEADSHELL_PLUGIN_INDEX` or the
/// default.
///
/// Two failures are kept apart (K9, D-043):
/// - If the catalog **could not be reached** it returns `Err(reason)`; the
///   caller skips the test.
/// - If it was reached but the plugin **could not be installed** it panics:
///   the index is broken, a file is missing or a hash does not match. This
///   must turn red — the user cannot install it either.
pub async fn install_from_catalog(config: &Config, name: &str) -> Result<PathBuf, String> {
    use headshell_core::diag::Stage;
    use headshell_core::plugin::catalog;

    let http = headshell_core::net::default_http_client().map_err(|err| err.chain_text())?;
    let index = config.plugin_index_url();
    let result = async {
        let catalog = catalog::fetch(http.as_ref(), &index).await?;
        catalog::install(config, http.as_ref(), &catalog, name).await
    }
    .await;
    match result {
        Ok(_) => Ok(config.plugins_dir().join(name)),
        Err(err) if err.stage() == Stage::NetworkRequest => Err(format!(
            "could not read the catalog ({index}): {}",
            err.chain_text().replace('\n', " ")
        )),
        Err(err) => panic!(
            "could not install {name} from the catalog ({index}):\n{}",
            err.chain_text()
        ),
    }
}
