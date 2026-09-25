//! The namespaced secret store (D-042).
//!
//! A single concept: `<data_dir>/secrets.json`, `0600` on unix, the keys split
//! into namespaces — `plugin:soundcloud`, `provider:navidrome`. A plugin sees
//! **only its own namespace**.
//!
//! There is no `keyring` dependency, and that is deliberate (D-021 → D-042):
//! a new dependency, and fragile on headless Linux. Since reading goes through
//! a single place, putting a keyring behind it later is a job limited to
//! changing this file.
//!
//! **Values do not go into the log or into `headshell diag`.** The
//! diagnostics report is text that gets copied and pasted (K9); it cannot
//! carry tokens. The only things handed out are key **names** and counts
//! ([`Secrets::describe`]).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

/// A plugin's namespace: `plugin:<name>`.
#[must_use]
pub fn plugin_namespace(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

/// Namespace → (key → value).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secrets {
    namespaces: BTreeMap<String, BTreeMap<String, String>>,
}

impl Secrets {
    /// Reads from the file. If there is no file, an **empty store** — that is not
    /// an error, it means no secret has been written yet. If it is corrupt, an
    /// error: silently returning empty would mean taking the user's credentials
    /// for "none" and asking for them again.
    ///
    /// # Errors
    /// If the file cannot be read or the JSON is corrupt.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(io_err(Stage::ConfigLoad, path, err)),
        };
        serde_json::from_str(&raw).map_err(|source| {
            Error::new(
                Stage::ConfigLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })
    }

    /// Writes to the file. Permissions `0600` on Unix.
    ///
    /// # Errors
    /// If the directory cannot be created or the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| io_err(Stage::ConfigLoad, parent, err))?;
        }
        let text = serde_json::to_string_pretty(&self.namespaces).map_err(|source| {
            Error::new(
                Stage::ConfigLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })?;
        std::fs::write(path, text).map_err(|err| io_err(Stage::ConfigLoad, path, err))?;
        restrict_permissions(path)
    }

    /// All the secrets of a namespace. An empty map if the namespace does not
    /// exist.
    #[must_use]
    pub fn namespace(&self, namespace: &str) -> BTreeMap<String, String> {
        self.namespaces.get(namespace).cloned().unwrap_or_default()
    }

    /// Writes a single secret.
    pub fn set(&mut self, namespace: &str, key: impl Into<String>, value: impl Into<String>) {
        self.namespaces
            .entry(namespace.to_owned())
            .or_default()
            .insert(key.into(), value.into());
    }

    /// Deletes a single secret. If the namespace becomes empty, it is deleted
    /// too.
    ///
    /// Returns: whether anything was really deleted.
    pub fn remove(&mut self, namespace: &str, key: &str) -> bool {
        let Some(entries) = self.namespaces.get_mut(namespace) else {
            return false;
        };
        let removed = entries.remove(key).is_some();
        if entries.is_empty() {
            self.namespaces.remove(namespace);
        }
        removed
    }

    /// Deletes a whole namespace. Returns: whether the namespace existed.
    pub fn remove_namespace(&mut self, namespace: &str) -> bool {
        self.namespaces.remove(namespace).is_some()
    }

    /// A safe summary for diagnostics and `--json`: namespace → **key names**.
    ///
    /// The values are left out on purpose (the K9 report must be copyable).
    #[must_use]
    pub fn describe(&self) -> BTreeMap<String, Vec<String>> {
        self.namespaces
            .iter()
            .map(|(ns, entries)| (ns.clone(), entries.keys().cloned().collect()))
            .collect()
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| io_err(Stage::ConfigLoad, path, err))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    // On Windows the counterpart is an ACL; it will be written when we get
    // there. We do not pass over it silently: the caller should know that
    // something was not done.
    tracing::warn!("secret file permissions were not restricted on this platform");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory must live as long as the file path: the two are returned
    /// together.
    fn temp_path(name: &str) -> (crate::test_support::TempDir, std::path::PathBuf) {
        let dir = crate::test_support::TempDir::new("secrets");
        let path = dir.join(name);
        (dir, path)
    }

    #[test]
    fn secrets_survive_a_round_trip_and_stay_private() {
        let (_dir, path) = temp_path("secrets.json");
        let mut secrets = Secrets::default();
        secrets.set(&plugin_namespace("soundcloud"), "client_id", "abc123");
        secrets.set("provider:navidrome", "token", "xyz");
        secrets.save(&path).unwrap();

        let back = Secrets::load(&path).unwrap();
        assert_eq!(back, secrets);
        assert_eq!(
            back.namespace("plugin:soundcloud").get("client_id"),
            Some(&"abc123".to_owned())
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "the secret file must be readable only by its owner"
            );
        }
    }

    #[test]
    fn a_plugin_sees_only_its_own_namespace() {
        let mut secrets = Secrets::default();
        secrets.set("plugin:soundcloud", "client_id", "abc");
        secrets.set("plugin:other", "client_id", "hidden");

        let mine = secrets.namespace("plugin:soundcloud");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine.get("client_id"), Some(&"abc".to_owned()));
        assert!(secrets.namespace("plugin:missing").is_empty());
    }

    #[test]
    fn describe_lists_key_names_but_never_values() {
        let mut secrets = Secrets::default();
        secrets.set("plugin:soundcloud", "client_id", "very-secret-value");
        let described = secrets.describe();
        let text = serde_json::to_string(&described).unwrap();
        assert!(text.contains("client_id"), "{text}");
        assert!(
            !text.contains("very-secret-value"),
            "the summary must not leak values: {text}"
        );
    }

    #[test]
    fn removing_the_last_key_drops_the_namespace() {
        let mut secrets = Secrets::default();
        secrets.set("plugin:a", "k", "v");
        assert!(secrets.remove("plugin:a", "k"));
        assert!(secrets.describe().is_empty());
        assert!(
            !secrets.remove("plugin:a", "k"),
            "a second removal must not lie"
        );
    }

    #[test]
    fn a_missing_file_is_empty_but_a_broken_one_is_an_error() {
        let (_dir, path) = temp_path("secrets.json");
        assert_eq!(Secrets::load(&path).unwrap(), Secrets::default());

        std::fs::write(&path, "{ broken").unwrap();
        let err = Secrets::load(&path).unwrap_err();
        assert_eq!(err.stage(), Stage::ConfigLoad);
    }
}
