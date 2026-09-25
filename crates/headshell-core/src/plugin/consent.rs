//! The plugin consent ledger: `<data_dir>/plugins.json` (D-040).
//!
//! The approved permission set is stored **as it is**, not a digest of it:
//! the user should be able to open the file and read what they said yes to,
//! and the answer to "what new thing is being asked?" should come from a set
//! difference, not a hash comparison.
//!
//! A plugin without consent **is not loaded but is visible**: `headshell
//! plugin list` shows it as "awaiting consent". A plugin skipped silently is
//! a plugin the user thinks is installed but that does not work (K9).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

use super::manifest::Permissions;

/// A plugin's consent record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginConsent {
    /// The permission set the user said yes to.
    pub granted: Permissions,
    pub granted_at: jiff::Timestamp,
    /// `false` if the user later disabled it. The record is not deleted:
    /// disabling is not forgetting, and re-enabling does not ask for the same
    /// permissions again.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

const fn default_true() -> bool {
    true
}

/// A plugin's consent state at this moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConsentStatus {
    /// Approved, and the requested permissions are within the consent.
    Approved,
    /// Never asked.
    NotAsked,
    /// There is consent, but the plugin asks for **more**. `extra` is the new
    /// requests.
    NeedsApproval { extra: Permissions },
    /// The user disabled it.
    Disabled,
}

impl ConsentStatus {
    /// Can the plugin be run.
    #[must_use]
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }

    /// A one-line reason to show the user.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Approved => "approved".to_owned(),
            Self::NotAsked => "awaiting consent — `headshell plugin approve <name>`".to_owned(),
            Self::NeedsApproval { extra } => format!(
                "asks for new permissions ({}) — `headshell plugin approve <name>`",
                extra.describe()
            ),
            Self::Disabled => "disabled — `headshell plugin enable <name>`".to_owned(),
        }
    }
}

/// The consent ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConsentStore {
    plugins: BTreeMap<String, PluginConsent>,
}

impl ConsentStore {
    /// Reads from the file. If there is no file, an empty ledger — meaning no
    /// plugin has been approved yet. If it is corrupt, an error: returning an
    /// empty ledger would silently delete every consent.
    ///
    /// # Errors
    /// If the file cannot be read or the JSON is corrupt.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(io_err(Stage::PluginLoad, path, err)),
        };
        serde_json::from_str(&raw).map_err(|source| {
            Error::new(
                Stage::PluginLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })
    }

    /// Writes to the file.
    ///
    /// # Errors
    /// If the directory cannot be created or the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| io_err(Stage::PluginLoad, parent, err))?;
        }
        let text = serde_json::to_string_pretty(&self.plugins).map_err(|source| {
            Error::new(
                Stage::PluginLoad,
                ErrorKind::Json {
                    entry: path.display().to_string(),
                    source,
                },
            )
        })?;
        std::fs::write(path, text).map_err(|err| io_err(Stage::PluginLoad, path, err))
    }

    /// The state for the permissions a plugin asks for.
    #[must_use]
    pub fn status(&self, name: &str, requested: &Permissions) -> ConsentStatus {
        let Some(record) = self.plugins.get(name) else {
            return ConsentStatus::NotAsked;
        };
        if !record.enabled {
            return ConsentStatus::Disabled;
        }
        if requested.is_covered_by(&record.granted) {
            ConsentStatus::Approved
        } else {
            ConsentStatus::NeedsApproval {
                extra: requested.beyond(&record.granted),
            }
        }
    }

    /// Approves the permissions (and enables the plugin if it was disabled). The
    /// approved set is **exactly what was asked**, not the union: if the plugin
    /// dropped a permission, the ledger should drop it too.
    pub fn approve(&mut self, name: &str, requested: &Permissions, now: jiff::Timestamp) {
        self.plugins.insert(
            name.to_owned(),
            PluginConsent {
                granted: requested.normalized(),
                granted_at: now,
                enabled: true,
            },
        );
    }

    /// Disables the plugin. The consent record is **kept**.
    ///
    /// Returns: whether there was a record.
    pub fn disable(&mut self, name: &str) -> bool {
        match self.plugins.get_mut(name) {
            Some(record) => {
                record.enabled = false;
                true
            }
            None => false,
        }
    }

    /// Re-enables a disabled plugin. Returns: whether there was a record.
    pub fn enable(&mut self, name: &str) -> bool {
        match self.plugins.get_mut(name) {
            Some(record) => {
                record.enabled = true;
                true
            }
            None => false,
        }
    }

    /// Forgets the consent entirely — it is asked from scratch on the next run.
    /// Returns: whether there was a record.
    pub fn forget(&mut self, name: &str) -> bool {
        self.plugins.remove(name).is_some()
    }

    /// The recorded consent (if any).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PluginConsent> {
        self.plugins.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(hosts: &[&str]) -> Permissions {
        Permissions {
            net: hosts.iter().map(|h| (*h).to_owned()).collect(),
        }
    }

    fn now() -> jiff::Timestamp {
        jiff::Timestamp::now()
    }

    #[test]
    fn an_unknown_plugin_has_not_been_asked_about() {
        let store = ConsentStore::default();
        assert_eq!(
            store.status("soundcloud", &net(&["a.example"])),
            ConsentStatus::NotAsked
        );
    }

    #[test]
    fn approval_covers_the_same_and_smaller_sets_but_not_bigger_ones() {
        let mut store = ConsentStore::default();
        store.approve("p", &net(&["a.example", "b.example"]), now());

        assert!(
            store
                .status("p", &net(&["a.example", "b.example"]))
                .is_approved()
        );
        assert!(store.status("p", &net(&["a.example"])).is_approved());

        let status = store.status("p", &net(&["a.example", "c.example"]));
        match status {
            ConsentStatus::NeedsApproval { extra } => {
                assert_eq!(extra.net, vec!["c.example".to_owned()]);
            }
            other => panic!("a grown permission set must ask for consent again: {other:?}"),
        }
    }

    #[test]
    fn disabling_keeps_the_record_so_re_enabling_asks_nothing() {
        let mut store = ConsentStore::default();
        store.approve("p", &net(&["a.example"]), now());
        assert!(store.disable("p"));
        assert_eq!(
            store.status("p", &net(&["a.example"])),
            ConsentStatus::Disabled
        );
        assert!(store.enable("p"));
        assert!(store.status("p", &net(&["a.example"])).is_approved());
    }

    #[test]
    fn forgetting_sends_the_plugin_back_to_the_start() {
        let mut store = ConsentStore::default();
        store.approve("p", &net(&["a.example"]), now());
        assert!(store.forget("p"));
        assert_eq!(
            store.status("p", &net(&["a.example"])),
            ConsentStatus::NotAsked
        );
        assert!(!store.forget("p"));
    }

    #[test]
    fn the_ledger_survives_a_file_round_trip() {
        let dir = crate::test_support::TempDir::new("consent");
        let path = dir.join("plugins.json");

        assert_eq!(ConsentStore::load(&path).unwrap(), ConsentStore::default());

        let mut store = ConsentStore::default();
        store.approve("p", &net(&["b.example", "a.example"]), now());
        store.save(&path).unwrap();

        let back = ConsentStore::load(&path).unwrap();
        assert_eq!(back, store);
        // The approved set must be written sorted; it is normalised so that a
        // change in order does not ask for consent again.
        assert_eq!(
            back.get("p").unwrap().granted.net,
            vec!["a.example".to_owned(), "b.example".to_owned()]
        );
    }

    #[test]
    fn a_broken_ledger_is_an_error_not_an_empty_one() {
        let dir = crate::test_support::TempDir::new("consent-broken");
        let path = dir.join("plugins.json");
        std::fs::write(&path, "{broken").unwrap();
        let err = ConsentStore::load(&path).unwrap_err();
        assert_eq!(err.stage(), Stage::PluginLoad);
    }
}
