//! Shared helpers for unit tests. Compiled only under `cfg(test)`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::Config;

/// A temporary directory that deletes itself.
///
/// Tests **delete** their temporary directories, even a failing test: `Drop`
/// runs during a panic too. Once they did not, and 1,100 directories, 1.2 GB,
/// piled up in a development machine's `/tmp` — a tmpfs there, that is,
/// memory (D-070).
///
/// The name is `headshell-<label>-<process>-<seq>`. The sequence is a counter
/// that grows within the process, not the clock: Windows' clock resolution
/// could give two parallel tests the same name. If an old directory with the
/// same name exists (from a killed run) it **is not used**, the next sequence
/// number is taken — so a test does not mistake someone else's leftovers for
/// its own data.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        loop {
            let dir = base.join(format!(
                "headshell-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => panic!(
                    "could not open a temporary directory ({}): {err}",
                    dir.display()
                ),
            }
        }
    }

    pub(crate) fn path(&self) -> &Path {
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
        // If it cannot be deleted (a file left open on Windows) the test is not
        // failed; but it does not stay silent either, what was left behind is
        // written out.
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

/// A `Config` on top of a data directory that deletes itself.
///
/// It derefs to `Config`: `&test_config` can be passed anywhere a `&Config`
/// is expected, and when the value is dropped the directory goes too.
pub(crate) struct TestConfig {
    _dir: TempDir,
    config: Config,
}

impl TestConfig {
    pub(crate) fn new(label: &str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temp_dir_is_unique_and_gone_after_drop() {
        let first = TempDir::new("support");
        let second = TempDir::new("support");
        assert_ne!(first.path(), second.path());
        std::fs::write(first.join("file"), "x").unwrap();
        let path = first.path().to_path_buf();
        drop(first);
        assert!(
            !path.exists(),
            "the directory was not deleted: {}",
            path.display()
        );
    }
}
