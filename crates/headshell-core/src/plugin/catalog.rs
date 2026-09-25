//! Plugin catalog: an `index.json` read from a separate repository (D-071).
//!
//! Plugins do not live in the main repository. They live as directories in
//! the [`headshell/plugins`] repository, and the `index.json` at that
//! repository's root is generated from them ([`build_index`]). The app reads
//! the list from this file and installs and updates plugins from there.
//!
//! [`headshell/plugins`]: https://github.com/headshell/plugins
//!
//! ## Index (schema 1)
//!
//! ```json
//! {
//!   "schema": 1,
//!   "url_template": "https://…/refs/tags/{name}-{version}/{name}/{path}",
//!   "plugins": [
//!     {
//!       "manifest": { "name": "soundcloud", "version": "0.2.0", … },
//!       "files": [
//!         { "path": "plugin.json", "url": "https://…", "sha256": "…" },
//!         { "path": "main.js", "url": "https://…", "sha256": "…" }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! An entry carries the plugin's manifest **as is**: the permissions shown in
//! the catalog and the permissions of the installed plugin come from the same
//! source, and the downloaded `plugin.json` is compared with it. File
//! addresses are free: today they all point at `headshell/plugins` release
//! tags, but a plugin in another repository can be listed the same way
//! (Obsidian's model). `url_template` is only the generator's note; the
//! client does not read it.
//!
//! ## Trust
//!
//! The index comes over HTTPS and carries every file's sha256:
//!
//! 1. Every file is verified against its hash; if one does not match,
//!    nothing is written to disk.
//! 2. The downloaded `plugin.json` must be **identical** to the manifest the
//!    index showed.
//! 3. An installed plugin **awaits consent** (D-040): coming from the catalog
//!    is not consent.
//!
//! Pinning the addresses to release tags is the generator's job, which is why
//! `{version}` is mandatory in the template: GitHub's raw content cache keeps
//! things for five minutes, and an address pinned to `main` could pair the
//! new index with the old file while a release goes out — a frightening,
//! transient "hash mismatch" error.
//!
//! ## Network
//!
//! The catalog is read only by **an explicit command**: `plugin catalog`,
//! `plugin install` (if the plugin is not on disk) and `plugin update`. No
//! request at startup or in the background — the same principle as
//! "importing an export never silently connects anyone to the network".
//!
//! ## Whose files get touched
//!
//! Every plugin installed from the catalog has an origin record in its
//! directory ([`ORIGIN_FILE`]): which catalog, which version, which files and
//! their hashes. An update touches **only** a plugin that has this record and
//! whose files match it. A plugin put there by hand (no record) or changed by
//! hand (hash mismatch) is never overwritten: a developer's working copy must
//! not be deleted by an update.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::net::{HttpClient, HttpRequest};

use super::artifact::{hash_file, sha256_hex, short_hash};
use super::manifest::{
    MANIFEST_FILE, Permissions, PluginManifest, Requirement, url_host, validate_catalog_name,
    validate_local_name,
};
use super::protocol::PLUGIN_API;

/// The default catalog: the index on the `main` branch of the
/// `headshell/plugins` repository.
///
/// The index lives on the branch, the files on release tags: the index
/// always gives the current list, while the files it points at never change.
pub const DEFAULT_INDEX_URL: &str =
    "https://raw.githubusercontent.com/headshell/plugins/refs/heads/main/index.json";

/// Environment variable to change the catalog address: a fork, a mirror, or a
/// local server for testing.
pub const INDEX_ENV: &str = "HEADSHELL_PLUGIN_INDEX";

/// Name of the index file at the catalog repository's root.
pub const INDEX_FILE: &str = "index.json";

/// The index format this core reads.
///
/// The rule is the same as `api`'s: **adding does not bump it**, removing or
/// changing a meaning does. An unknown schema is not read, and the reason is
/// given.
pub const INDEX_SCHEMA: u32 = 1;

/// The origin record in the directory of a plugin installed from the
/// catalog.
pub const ORIGIN_FILE: &str = "origin.json";

/// The largest index size accepted. A safety belt: a wrong address must not
/// fill memory.
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;

/// The largest plugin file size accepted. Today's largest plugin is ~12 KB.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Names the engine uses inside a plugin directory: a catalog file cannot
/// write to them. `state/` is the store the engine keeps on the plugin's
/// behalf ([`Config::plugin_state_dir`]), `origin.json` the origin record.
const RESERVED: &[&str] = &[ORIGIN_FILE, "state"];

/// Resolves the catalog address from the environment:
/// `HEADSHELL_PLUGIN_INDEX` or the default. Called from outside through
/// [`Config::plugin_index_url`]; it is not exported because the environment is
/// passed as a closure (K7).
#[must_use]
pub(crate) fn resolve_index_url(env: &dyn Fn(&str) -> Option<String>) -> String {
    env(INDEX_ENV)
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_INDEX_URL.to_owned())
}

/// A plugin file in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFile {
    /// Path relative to the plugin directory, `/`-separated (`main.js`).
    pub path: String,
    /// Where to download it from.
    pub url: String,
    /// Expected sha256, lower-case hex. If it does not match, the file is not
    /// written.
    pub sha256: String,
}

/// A catalog plugin's state on this machine. Measured **without going
/// online**; "update available" is a comparison with the catalog's version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum InstallState {
    /// Not on this machine.
    NotInstalled,
    /// Installed from the catalog, same version as the catalog, files as
    /// installed.
    Current { version: String },
    /// Installed from the catalog; the catalog has another version.
    UpdateAvailable {
        installed: String,
        available: String,
    },
    /// The directory exists but has no origin record: put there by hand. Updates
    /// leave it alone.
    Manual,
    /// Installed from the catalog, but its files are not as installed. Updates
    /// leave it alone — a manual change must not be deleted.
    Modified { version: String, files: Vec<String> },
    /// The origin record could not be read. Updates leave it alone.
    Unreadable { detail: String },
}

impl InstallState {
    /// Is there a plugin with this name on disk.
    #[must_use]
    pub const fn is_installed(&self) -> bool {
        !matches!(self, Self::NotInstalled)
    }

    /// One line to show the user.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NotInstalled => "not installed".to_owned(),
            Self::Current { version } => format!("installed {version} · up to date"),
            Self::UpdateAvailable {
                installed,
                available,
            } => format!("update available: {installed} → {available}"),
            Self::Manual => {
                "installed by hand — the catalog does not touch it (no origin record)".to_owned()
            }
            Self::Modified { version, files } => format!(
                "installed {version} · changed locally ({}) — an update does not overwrite it",
                files.join(", ")
            ),
            Self::Unreadable { detail } => format!("origin record could not be read: {detail}"),
        }
    }
}

/// A plugin in the catalog — as shown to the user.
///
/// Entries that cannot be installed are here too: an entry that silently
/// drops out is a plugin the user looked for in the catalog and did not find
/// (K9). The reason is in `problem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPlugin {
    pub name: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub api: Option<u32>,
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The network permissions the plugin declares — the ones that will be up
    /// for approval once installed.
    #[serde(default)]
    pub permissions: Permissions,
    /// The tools the engine will install (D-055). Shown apart from the network
    /// permissions.
    #[serde(default)]
    pub requires: Vec<Requirement>,
    #[serde(default)]
    pub files: Vec<CatalogFile>,
    /// Why it cannot be installed, if it cannot; one line.
    pub problem: Option<String>,
    pub installed: InstallState,
}

/// Summary of one catalog read (K9: how many came, in what state).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSummary {
    /// Number of entries in the index.
    pub listed: usize,
    /// Entries this version can install.
    pub installable: usize,
    /// Those installed on this machine (including ones put there by hand).
    pub installed: usize,
    /// Those with an update.
    pub updates: usize,
    /// Entries that cannot be installed (broken, incompatible version).
    pub problems: usize,
}

impl CatalogSummary {
    /// Copies the counters into the diagnostics recorder.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
        recorder.set("catalog.listed", n(self.listed));
        recorder.set("catalog.installable", n(self.installable));
        recorder.set("catalog.installed", n(self.installed));
        recorder.set("catalog.updates", n(self.updates));
        recorder.set("catalog.problems", n(self.problems));
    }
}

/// The catalog as read against this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSurvey {
    pub plugins: Vec<CatalogPlugin>,
    /// Plugins installed **from this catalog** that are no longer listed. If a
    /// plugin was pulled from the catalog, the user should know.
    pub delisted: Vec<String>,
    pub summary: CatalogSummary,
}

/// The origin record of a plugin installed from the catalog
/// ([`ORIGIN_FILE`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRecord {
    /// The catalog it was read from.
    pub index: String,
    pub version: String,
    pub installed_at: jiff::Timestamp,
    /// Path → sha256. Updates compare against these to catch local changes.
    pub files: BTreeMap<String, String>,
}

/// What this command downloaded from the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFetch {
    pub index: String,
    pub version: String,
    pub files: Vec<CatalogFile>,
}

/// A tool (an artifact the engine installs) changing in an update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolChange {
    pub name: String,
    /// The previous version; `None`: newly added.
    pub from: Option<String>,
    /// The new version; `None`: no longer requested.
    pub to: Option<String>,
    /// Whether this platform's binary changed its address or hash — even with
    /// the same version.
    pub binary_changed: bool,
}

impl ToolChange {
    /// One line to show the user.
    #[must_use]
    pub fn describe(&self) -> String {
        match (&self.from, &self.to) {
            (None, Some(to)) => format!("{} {to} added", self.name),
            (Some(from), None) => format!("{} {from} no longer requested", self.name),
            (Some(from), Some(to)) if from != to => format!("{} {from} → {to}", self.name),
            (Some(version), Some(_)) => format!(
                "{} {version}: same version, this platform's binary changed",
                self.name
            ),
            (None, None) => self.name.clone(),
        }
    }
}

/// The result of one update attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// The new version was put in place.
    Updated {
        from: String,
        to: String,
        /// Network permissions the new version asks for **in addition**. If not
        /// empty, the plugin awaits consent again (D-040).
        permissions_added: Permissions,
        /// Changes to the tools the engine installs. They need no consent (D-071,
        /// the user's decision) but are **stated**: an unconfined binary must not
        /// change silently.
        tools_changed: Vec<ToolChange>,
    },
    /// The installed version is the same as the catalog's.
    Current { version: String },
    /// Left alone; the reason is written.
    Skipped { reason: String },
    /// Tried and failed (network, hash, disk). The plugin is as it was.
    Failed { error: String },
}

impl UpdateOutcome {
    /// One line to show the user.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Updated { from, to, .. } => format!("updated: {from} → {to}"),
            Self::Current { version } => format!("up to date ({version})"),
            Self::Skipped { reason } => format!("skipped — {reason}"),
            Self::Failed { error } => format!("NOT UPDATED — {error}"),
        }
    }
}

/// Summary of one update round (K9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSummary {
    pub checked: usize,
    pub updated: usize,
    pub current: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl UpdateSummary {
    /// Counts from the results.
    #[must_use]
    pub fn of<'a>(outcomes: impl IntoIterator<Item = &'a UpdateOutcome>) -> Self {
        let mut summary = Self::default();
        for outcome in outcomes {
            summary.checked += 1;
            match outcome {
                UpdateOutcome::Updated { .. } => summary.updated += 1,
                UpdateOutcome::Current { .. } => summary.current += 1,
                UpdateOutcome::Skipped { .. } => summary.skipped += 1,
                UpdateOutcome::Failed { .. } => summary.failed += 1,
            }
        }
        summary
    }

    /// Copies the counters into the diagnostics recorder.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
        recorder.set("update.checked", n(self.checked));
        recorder.set("update.updated", n(self.updated));
        recorder.set("update.current", n(self.current));
        recorder.set("update.skipped", n(self.skipped));
        recorder.set("update.failed", n(self.failed));
    }
}

/// A removed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub path: PathBuf,
    /// The target, if the directory was a symbolic link. Only the **link** was
    /// removed; the files at the target were not touched — it may be a
    /// developer's working copy.
    pub link_target: Option<PathBuf>,
}

/// A plugin that went into the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedPlugin {
    pub name: String,
    pub version: String,
    pub files: Vec<CatalogFile>,
}

/// A generated index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltIndex {
    /// The text to write to the file (with a trailing newline).
    pub json: String,
    pub plugins: Vec<IndexedPlugin>,
}

/// An entry in the index — validated, or why it could not be.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogEntry {
    name: String,
    display_name: Option<String>,
    version: Option<String>,
    api: Option<u32>,
    description: Option<String>,
    files: Vec<CatalogFile>,
    /// Present only if the entry can be installed.
    manifest: Option<PluginManifest>,
    problem: Option<String>,
}

/// A catalog that has been read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    index: String,
    entries: Vec<CatalogEntry>,
}

/// The smallest form of the index file that asks for its schema.
#[derive(Deserialize)]
struct SchemaProbe {
    schema: Option<u32>,
}

#[derive(Deserialize)]
struct RawIndex {
    #[serde(default)]
    plugins: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct IndexOut<'a> {
    schema: u32,
    url_template: &'a str,
    plugins: Vec<EntryOut>,
}

#[derive(Serialize)]
struct EntryOut {
    manifest: serde_json::Value,
    files: Vec<CatalogFile>,
}

impl Catalog {
    /// The address it was read from.
    #[must_use]
    pub fn index(&self) -> &str {
        &self.index
    }

    /// Entry names, in index order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    /// Reads the index text.
    ///
    /// A broken **entry** does not stop the read: its reason is written to that
    /// entry's `problem` and the rest is read. A broken **index** (not JSON, an
    /// unknown schema) is an error.
    ///
    /// # Errors
    /// If the text is not JSON, has no schema, or has a schema other than the
    /// one this core reads.
    pub fn parse(index: &str, body: &[u8]) -> Result<Self> {
        let probe: SchemaProbe = serde_json::from_slice(body).map_err(|err| {
            catalog_err(
                index,
                format!("the index could not be read as JSON: {err} — does the address point at an index?"),
            )
        })?;
        match probe.schema {
            Some(INDEX_SCHEMA) => {}
            Some(schema) => {
                return Err(catalog_err(
                    index,
                    format!(
                        "the index format differs from what this version reads (schema {schema}, this version \
                         reads {INDEX_SCHEMA}) — update headshell"
                    ),
                ));
            }
            None => {
                return Err(catalog_err(
                    index,
                    "the index has no `schema` field — this is not a headshell plugin index"
                        .to_owned(),
                ));
            }
        }
        let raw: RawIndex = serde_json::from_slice(body).map_err(|err| {
            catalog_err(
                index,
                format!("the index's `plugins` list could not be read: {err}"),
            )
        })?;

        let loopback = url_host(index).is_ok_and(|host| is_loopback(&host));
        let mut entries: Vec<CatalogEntry> = raw
            .plugins
            .into_iter()
            .map(|value| parse_entry(index, value, loopback))
            .collect();

        // The same name twice: which one to install is not guessed (K9).
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for entry in &entries {
            *counts.entry(entry.name.clone()).or_default() += 1;
        }
        for entry in &mut entries {
            if counts.get(&entry.name).is_some_and(|count| *count > 1) {
                entry.manifest = None;
                entry.problem = Some(
                    "this name appears more than once in the catalog — which one to install is not guessed"
                        .to_owned(),
                );
            }
        }
        Ok(Self {
            index: index.to_owned(),
            entries,
        })
    }

    fn lookup(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// The named entry; if missing, which names exist and perhaps the one that was
    /// meant.
    fn find(&self, name: &str) -> Result<&CatalogEntry> {
        if let Some(entry) = self.lookup(name) {
            return Ok(entry);
        }
        let hint = self
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| format!(" — did you mean `{}`?", entry.name))
            .unwrap_or_default();
        let available = if self.entries.is_empty() {
            "the catalog is empty".to_owned()
        } else {
            format!("in the catalog: {}", self.names().join(", "))
        };
        Err(Error::new(
            Stage::PluginCatalog,
            ErrorKind::NotFound {
                what: format!(
                    "plugin named `{name}` in the catalog ({}){hint}; {available}",
                    self.index
                ),
            },
        ))
    }

    /// Reads the catalog against this machine: whether each entry is installed,
    /// whether it has an update, and installed plugins pulled from the catalog.
    /// **Does not go online.**
    ///
    /// # Errors
    /// If the plugin directory exists but cannot be read.
    pub fn survey(&self, config: &Config) -> Result<CatalogSurvey> {
        let plugins: Vec<CatalogPlugin> = self
            .entries
            .iter()
            .map(|entry| entry.view(state_of(config, entry)))
            .collect();

        let mut summary = CatalogSummary {
            listed: plugins.len(),
            ..CatalogSummary::default()
        };
        for plugin in &plugins {
            if plugin.problem.is_some() {
                summary.problems += 1;
            } else {
                summary.installable += 1;
            }
            if plugin.installed.is_installed() {
                summary.installed += 1;
            }
            if plugin.problem.is_none()
                && matches!(plugin.installed, InstallState::UpdateAvailable { .. })
            {
                summary.updates += 1;
            }
        }

        Ok(CatalogSurvey {
            plugins,
            delisted: self.delisted(config)?,
            summary,
        })
    }

    /// Plugins installed from this catalog that are no longer listed.
    fn delisted(&self, config: &Config) -> Result<Vec<String>> {
        let mut delisted = Vec::new();
        for (name, dir) in installed_dirs(config)? {
            if self.lookup(&name).is_some() {
                continue;
            }
            match read_record(&dir) {
                Ok(Some(record)) if record.index == self.index => delisted.push(name),
                Ok(_) => {}
                Err(err) => tracing::warn!(
                    plugin = %name,
                    error = %err.chain_text().replace('\n', " "),
                    "origin record could not be read; cannot tell whether it was pulled from the catalog"
                ),
            }
        }
        Ok(delisted)
    }

    /// The plugins an update looks at: those installed on this machine whose
    /// names are in the catalog, and those installed from this catalog and since
    /// pulled from the list.
    ///
    /// A plugin installed by hand whose name is not in the catalog (a developer's
    /// working copy) is not listed — it has nothing to do with the catalog.
    ///
    /// # Errors
    /// If the plugin directory exists but cannot be read.
    pub fn update_candidates(&self, config: &Config) -> Result<Vec<String>> {
        let mut names: BTreeSet<String> = installed_dirs(config)?
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| self.lookup(name).is_some())
            .collect();
        names.extend(self.delisted(config)?);
        Ok(names.into_iter().collect())
    }
}

impl CatalogEntry {
    fn view(&self, installed: InstallState) -> CatalogPlugin {
        let manifest = self.manifest.as_ref();
        CatalogPlugin {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            version: self.version.clone(),
            api: self.api,
            description: self.description.clone(),
            capabilities: manifest.map(|m| m.capabilities.clone()).unwrap_or_default(),
            permissions: manifest.map(|m| m.permissions.clone()).unwrap_or_default(),
            requires: manifest.map(|m| m.requires.clone()).unwrap_or_default(),
            files: self.files.clone(),
            problem: self.problem.clone(),
            installed,
        }
    }

    /// The installable manifest; otherwise an error with the reason.
    fn installable(&self, index: &str) -> Result<&PluginManifest> {
        match (&self.manifest, &self.problem) {
            (Some(manifest), None) => Ok(manifest),
            (_, Some(problem)) => Err(catalog_err(
                index,
                format!("{} cannot be installed: {problem}", self.name),
            )),
            (None, None) => Err(catalog_err(
                index,
                format!("{} cannot be installed: entry was not validated", self.name),
            )),
        }
    }
}

/// Reads one entry of the index. Never fails: problems go into `problem`.
fn parse_entry(index: &str, value: serde_json::Value, loopback: bool) -> CatalogEntry {
    let manifest_value = value.get("manifest").cloned();
    let field = |key: &str| {
        manifest_value
            .as_ref()
            .and_then(|manifest| manifest.get(key))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let api = manifest_value
        .as_ref()
        .and_then(|manifest| manifest.get("api"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|api| u32::try_from(api).ok());
    let files: std::result::Result<Vec<CatalogFile>, String> = value
        .get("files")
        .cloned()
        .ok_or_else(|| "no `files`".to_owned())
        .and_then(|files| {
            serde_json::from_value::<Vec<CatalogFile>>(files)
                .map_err(|err| format!("`files` could not be read: {err}"))
        })
        .map(|files| {
            files
                .into_iter()
                .map(|file| CatalogFile {
                    sha256: file.sha256.trim().to_ascii_lowercase(),
                    ..file
                })
                .collect()
        });

    let mut entry = CatalogEntry {
        name: field("name").unwrap_or_default(),
        display_name: field("display_name"),
        version: field("version"),
        api,
        description: field("description"),
        files: files.clone().unwrap_or_default(),
        manifest: None,
        problem: None,
    };

    let checked = (|| -> std::result::Result<PluginManifest, String> {
        let manifest_value = manifest_value.ok_or_else(|| "no `manifest`".to_owned())?;
        validate_catalog_name(&entry.name)?;
        if let Some(api) = entry.api
            && api != PLUGIN_API
        {
            return Err(format!(
                "incompatible with this headshell version: plugin api {api}, core api {PLUGIN_API}"
            ));
        }
        let origin = PathBuf::from(format!("{index}#{}", entry.name));
        let manifest = PluginManifest::parse(&manifest_value.to_string(), &entry.name, &origin)
            .map_err(|err| cause_text(&err))?;
        let version = manifest.version.as_deref().ok_or_else(|| {
            "no `version` — every plugin in the catalog must be versioned".to_owned()
        })?;
        validate_version(version)?;
        validate_files(&files?, &manifest, loopback)?;
        Ok(manifest)
    })();
    match checked {
        Ok(manifest) => entry.manifest = Some(manifest),
        Err(problem) => entry.problem = Some(problem),
    }
    entry
}

/// The entry's file list: exactly `plugin.json` and the script, each with a
/// valid path, hash and address.
fn validate_files(
    files: &[CatalogFile],
    manifest: &PluginManifest,
    loopback: bool,
) -> std::result::Result<(), String> {
    let mut seen = BTreeSet::new();
    for file in files {
        validate_file_path(&file.path)?;
        if !seen.insert(file.path.as_str()) {
            return Err(format!("`{}` appears twice in the file list", file.path));
        }
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "`{}`: `sha256` must be 64 hex digits (found: {} digits)",
                file.path,
                file.sha256.len()
            ));
        }
        check_url(&file.url, loopback).map_err(|detail| format!("`{}`: {detail}", file.path))?;
    }
    let main = manifest.main_file();
    let expected: BTreeSet<&str> = [MANIFEST_FILE, main.as_str()].into_iter().collect();
    if seen != expected {
        return Err(format!(
            "the file list must be exactly `{}`; found: `{}`",
            expected.into_iter().collect::<Vec<_>>().join("`, `"),
            seen.into_iter().collect::<Vec<_>>().join("`, `")
        ));
    }
    Ok(())
}

/// Whether a catalog file path stays inside the plugin directory, is plain,
/// and can go into an address without percent-encoding.
fn validate_file_path(path: &str) -> std::result::Result<(), String> {
    let invalid = || {
        Err(format!(
            "`{path}` is not a valid plugin file path: it must be relative, `/`-separated, each part \
             ASCII letters/digits/`.`/`-`/`_` and not starting with a dot"
        ))
    };
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return invalid();
    }
    for segment in path.split('/') {
        let valid = !segment.is_empty()
            && !segment.starts_with('.')
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !valid {
            return invalid();
        }
    }
    let top = path.split('/').next().unwrap_or(path);
    if RESERVED.contains(&top) {
        return Err(format!(
            "`{path}`: `{top}` is a name the engine uses in the plugin directory"
        ));
    }
    Ok(())
}

/// A catalog version goes into the address; it must not need percent-encoding.
fn validate_version(version: &str) -> std::result::Result<(), String> {
    let valid = !version.is_empty()
        && version.len() <= 64
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "version `{version}` cannot be used in the catalog: 1–64 characters, ASCII letters/digits/`.`/`-`/`_`/`+`"
        ))
    }
}

/// Whether an address is acceptable for the catalog: `https://`, or plain
/// `http://` only to this machine itself (`127.0.0.1`, `localhost`) and only if
/// the index is there too.
///
/// The index is the root of trust — it carries the files' hashes. An index
/// over plain HTTP can be changed by anyone on the path; the loopback address
/// has no such path, and that is where tests and local mirrors run.
fn check_url(url: &str, loopback_allowed: bool) -> std::result::Result<(), String> {
    let host = url_host(url)?;
    let https = url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    if https || (loopback_allowed && is_loopback(&host)) {
        Ok(())
    } else {
        Err(format!(
            "only https:// addresses are accepted (plain http only for this machine itself: \
             127.0.0.1, localhost) — found: {url}"
        ))
    }
}

fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "localhost"
}

/// The error's cause without its stage line: written into an entry's
/// `problem`, where `STEP: …` is noise.
fn cause_text(err: &Error) -> String {
    let mut parts = Vec::new();
    let mut current: Option<&dyn std::error::Error> = Some(err.kind());
    while let Some(cause) = current {
        parts.push(cause.to_string());
        current = cause.source();
    }
    parts.join(": ")
}

fn catalog_err(index: &str, detail: String) -> Error {
    Error::new(
        Stage::PluginCatalog,
        ErrorKind::PluginCatalog {
            index: index.to_owned(),
            detail,
        },
    )
}

/// Reads the catalog.
///
/// # Errors
/// If the address is not accepted, the network cannot be reached
/// (`NETWORK_REQUEST`), or the index is missing or unreadable
/// (`PLUGIN_CATALOG`).
pub async fn fetch(http: &dyn HttpClient, index: &str) -> Result<Catalog> {
    check_url(index, true).map_err(|detail| catalog_err(index, detail))?;
    let request = HttpRequest::get(index);
    let response = http.send(&request).await?;
    if response.status == 404 || response.status == 410 {
        return Err(catalog_err(
            index,
            format!(
                "index not found (HTTP {}) — is the address right? If `{INDEX_ENV}` is set, \
                 check it",
                response.status
            ),
        ));
    }
    response.error_for_status(index)?;
    if response.body.len() > MAX_INDEX_BYTES {
        return Err(catalog_err(
            index,
            format!(
                "the index is {} bytes, the limit is {MAX_INDEX_BYTES} — the address may not point at an index",
                response.body.len()
            ),
        ));
    }
    Catalog::parse(index, &response.body)
}

/// Installs a plugin from the catalog: downloads it, verifies its hashes and
/// its manifest, and puts it in place **in one step** together with the
/// origin record.
///
/// The files are first written to a temporary directory in the data
/// directory and moved as a directory: an interrupted install never leaves a
/// half plugin in the plugin directory. Once installed, the plugin **awaits
/// consent** (D-040).
///
/// # Errors
/// If the plugin is not in the catalog or cannot be installed, a directory
/// with this name already exists, a download or hash fails, or the files
/// cannot be written.
pub async fn install(
    config: &Config,
    http: &dyn HttpClient,
    catalog: &Catalog,
    name: &str,
) -> Result<CatalogFetch> {
    let entry = catalog.find(name)?;
    let manifest = entry.installable(&catalog.index)?;
    let dir = config.plugins_dir().join(&entry.name);
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(catalog_err(
            &catalog.index,
            format!(
                "{} is already installed ({}) — to update it, `headshell plugin update {}`",
                entry.name,
                dir.display(),
                entry.name
            ),
        ));
    }
    let version = manifest.version.clone().unwrap_or_default();

    let files = download(http, &catalog.index, entry).await?;
    verify_manifest(&catalog.index, entry, manifest, &files)?;

    let plugins_dir = config.plugins_dir();
    std::fs::create_dir_all(&plugins_dir)
        .map_err(|err| io_err(Stage::PluginCatalog, &plugins_dir, err))?;
    let staging = Staging::create(config.data_dir(), &entry.name)?;
    for (file, bytes) in &files {
        write_file(&relative(staging.path(), &file.path), bytes)?;
    }
    write_record(
        staging.path(),
        &InstallRecord {
            index: catalog.index.clone(),
            version: version.clone(),
            installed_at: jiff::Timestamp::now(),
            files: record_files(&files),
        },
    )?;
    staging.move_to(&dir, &catalog.index)?;

    tracing::info!(plugin = %entry.name, version = %version, catalog = %catalog.index, "plugin installed from the catalog");
    Ok(CatalogFetch {
        index: catalog.index.clone(),
        version,
        files: entry.files.clone(),
    })
}

/// Brings a plugin installed from the catalog to the catalog's version.
///
/// Touches only a plugin that has an origin record and whose files match it;
/// for the others it returns [`UpdateOutcome::Skipped`] with the reason.
/// Files are replaced one by one, each atomically, and the origin record is
/// written last; `state/` (the plugin's persistent store) is kept.
///
/// `platform` is the platform whose binary tool changes are reported for
/// ([`super::artifact::current_platform`]).
///
/// # Errors
/// If the plugin is not installed, a download or hash fails, or the files
/// cannot be written. An interrupted update is completed by the next one: a
/// file that carries the catalog's new hash does not count as a "local
/// change".
pub async fn update(
    config: &Config,
    http: &dyn HttpClient,
    catalog: &Catalog,
    name: &str,
    platform: &str,
) -> Result<UpdateOutcome> {
    let dir = installed_dir(config, name)?;
    let Some(entry) = catalog.lookup(name) else {
        return Ok(UpdateOutcome::Skipped {
            reason: format!(
                "not in the catalog ({}) — it may have been pulled from the catalog",
                catalog.index
            ),
        });
    };
    let (installed, available) = match state_of(config, entry) {
        InstallState::UpdateAvailable {
            installed,
            available,
        } => (installed, available),
        InstallState::Current { version } => return Ok(UpdateOutcome::Current { version }),
        InstallState::Manual => {
            return Ok(UpdateOutcome::Skipped {
                reason: format!(
                    "installed by hand (no origin record) — an update does not overwrite files put there by \
                     hand; to install the catalog's version, first `headshell plugin remove {name}`"
                ),
            });
        }
        InstallState::Modified { files, .. } => {
            return Ok(UpdateOutcome::Skipped {
                reason: format!(
                    "changed locally ({}) — not overwritten; to go back to the catalog's version, \
                     `headshell plugin remove {name}` and `install`",
                    files.join(", ")
                ),
            });
        }
        InstallState::Unreadable { detail } => {
            return Ok(UpdateOutcome::Skipped {
                reason: format!("origin record could not be read: {detail}"),
            });
        }
        InstallState::NotInstalled => {
            return Ok(UpdateOutcome::Skipped {
                reason: "not installed".to_owned(),
            });
        }
    };
    if let Some(problem) = &entry.problem {
        return Ok(UpdateOutcome::Skipped {
            reason: format!("the catalog's version ({available}) cannot be installed: {problem}"),
        });
    }
    let manifest = entry.installable(&catalog.index)?;
    let previous = read_record(&dir)?;
    // If the old manifest cannot be read the update still goes ahead (that is
    // how a broken version gets fixed); the comparison is made against an empty
    // predecessor.
    let old_manifest = PluginManifest::load(&dir).ok();

    let files = download(http, &catalog.index, entry).await?;
    verify_manifest(&catalog.index, entry, manifest, &files)?;

    // Script first, manifest next, origin record last: if interrupted, the new
    // script running with the old manifest's permissions can only stay within
    // the old permissions, and the old record triggers the next update.
    let mut ordered: Vec<&(CatalogFile, Vec<u8>)> = files.iter().collect();
    ordered.sort_by_key(|(file, _)| file.path == MANIFEST_FILE);
    for (file, bytes) in ordered {
        replace_file(&dir, &file.path, bytes)?;
    }
    if let Some(previous) = &previous {
        for path in previous.files.keys() {
            if files.iter().any(|(file, _)| &file.path == path) {
                continue;
            }
            let stale = relative(&dir, path);
            match std::fs::remove_file(&stale) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(io_err(Stage::PluginCatalog, &stale, err)),
            }
        }
    }
    write_record(
        &dir,
        &InstallRecord {
            index: catalog.index.clone(),
            version: available.clone(),
            installed_at: jiff::Timestamp::now(),
            files: record_files(&files),
        },
    )?;

    let (old_permissions, old_requires) = old_manifest
        .map(|old| (old.permissions, old.requires))
        .unwrap_or_default();
    tracing::info!(plugin = %name, previous = %installed, new = %available, "plugin updated");
    Ok(UpdateOutcome::Updated {
        from: installed,
        to: available,
        permissions_added: manifest.permissions.beyond(&old_permissions),
        tools_changed: tool_changes(&old_requires, &manifest.requires, platform),
    })
}

/// Removes a plugin from disk: its directory, together with the `state/`
/// inside.
///
/// If the directory is a symbolic link, **only the link** is removed and its
/// target is not touched. The consent record and secrets are handled by the
/// caller ([`crate::session`]), not here.
///
/// # Errors
/// If the name is not a single directory name, the plugin is not installed,
/// or it cannot be deleted.
pub fn remove(config: &Config, name: &str) -> Result<Removed> {
    let dir = installed_dir(config, name)?;
    let meta =
        std::fs::symlink_metadata(&dir).map_err(|err| io_err(Stage::PluginCatalog, &dir, err))?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(&dir).ok();
        remove_link(&dir)?;
        return Ok(Removed {
            path: dir,
            link_target: target,
        });
    }
    if !meta.is_dir() {
        return Err(Error::new(
            Stage::PluginCatalog,
            ErrorKind::InvalidInput {
                detail: format!("{} is not a plugin directory", dir.display()),
            },
        ));
    }
    std::fs::remove_dir_all(&dir).map_err(|err| io_err(Stage::PluginCatalog, &dir, err))?;
    Ok(Removed {
        path: dir,
        link_target: None,
    })
}

/// An installed plugin's directory.
///
/// The name is validated before it is joined to a path
/// ([`validate_local_name`]): a name carrying `../` would reach outside the
/// data directory. If it is not installed, the error says **what to do**.
///
/// # Errors
/// If the name is not a single directory name or there is no plugin with
/// this name.
pub fn installed_dir(config: &Config, name: &str) -> Result<PathBuf> {
    validate_local_name(name)
        .map_err(|detail| Error::new(Stage::PluginCatalog, ErrorKind::InvalidInput { detail }))?;
    let dir = config.plugins_dir().join(name);
    match std::fs::symlink_metadata(&dir) {
        Ok(_) => Ok(dir),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(Error::new(
            Stage::PluginCatalog,
            ErrorKind::NotFound {
                what: format!(
                    "installed plugin: {name} ({}) — to install it, `headshell plugin install {name}`",
                    dir.display()
                ),
            },
        )),
        Err(err) => Err(io_err(Stage::PluginCatalog, &dir, err)),
    }
}

/// Removes a symbolic link. On Unix a link is a file; on Windows a directory
/// link needs `remove_dir`. Neither touches the target.
fn remove_link(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(first) => {
            std::fs::remove_dir(path).map_err(|_| io_err(Stage::PluginCatalog, path, first))
        }
    }
}

/// Reads a plugin's origin record. **No** record is `None`: the plugin was
/// put there by hand, which is not an error.
///
/// # Errors
/// If the record exists but cannot be read or is corrupt.
pub fn read_record(dir: &Path) -> Result<Option<InstallRecord>> {
    let path = dir.join(ORIGIN_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(io_err(Stage::PluginCatalog, &path, err)),
    };
    serde_json::from_str(&raw).map(Some).map_err(|source| {
        Error::new(
            Stage::PluginCatalog,
            ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })
}

/// A catalog entry's state on this machine. Does not go online.
fn state_of(config: &Config, entry: &CatalogEntry) -> InstallState {
    let dir = config.plugins_dir().join(&entry.name);
    match std::fs::symlink_metadata(&dir) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return InstallState::NotInstalled;
        }
        Err(err) => {
            return InstallState::Unreadable {
                detail: format!("{}: {err}", dir.display()),
            };
        }
    }
    let record = match read_record(&dir) {
        Ok(Some(record)) => record,
        Ok(None) => return InstallState::Manual,
        Err(err) => {
            return InstallState::Unreadable {
                detail: cause_text(&err),
            };
        }
    };
    let modified = modified_files(&dir, &record, &entry.files);
    if !modified.is_empty() {
        return InstallState::Modified {
            version: record.version,
            files: modified,
        };
    }
    match &entry.version {
        Some(available) if *available != record.version => InstallState::UpdateAvailable {
            installed: record.version,
            available: available.clone(),
        },
        _ => InstallState::Current {
            version: record.version,
        },
    }
}

/// Files that are not as installed. A file carrying the catalog's **new**
/// hash does not count as changed: it is the trace of an interrupted update,
/// and the next update completes it.
fn modified_files(
    dir: &Path,
    record: &InstallRecord,
    catalog_files: &[CatalogFile],
) -> Vec<String> {
    record
        .files
        .iter()
        .filter(|(path, recorded)| match hash_file(&relative(dir, path)) {
            Ok(found) => {
                found != **recorded
                    && !catalog_files
                        .iter()
                        .any(|file| &file.path == *path && file.sha256 == found)
            }
            // A deleted or unreadable file is not as installed either.
            Err(_) => true,
        })
        .map(|(path, _)| path.clone())
        .collect()
}

/// Subdirectories of the plugin directory: `(name, path)`. Empty if the
/// directory does not exist.
fn installed_dirs(config: &Config) -> Result<Vec<(String, PathBuf)>> {
    let plugins_dir = config.plugins_dir();
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(io_err(Stage::PluginCatalog, &plugins_dir, err)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| io_err(Stage::PluginCatalog, &plugins_dir, err))?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push((entry.file_name().to_string_lossy().into_owned(), path));
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Downloads the entry's files and verifies the hash of **every one**. If one
/// does not match, nothing is returned — and nothing is written to disk.
async fn download(
    http: &dyn HttpClient,
    index: &str,
    entry: &CatalogEntry,
) -> Result<Vec<(CatalogFile, Vec<u8>)>> {
    let mut out = Vec::with_capacity(entry.files.len());
    for file in &entry.files {
        let request = HttpRequest::get(&file.url);
        let response = http.send(&request).await?;
        if response.status == 404 || response.status == 410 {
            return Err(catalog_err(
                index,
                format!(
                    "{} {}: file not found (HTTP {}, {}) — the index points at a version that does not \
                     exist; fixing it is the catalog maintainer's job",
                    entry.name, file.path, response.status, file.url
                ),
            ));
        }
        response.error_for_status(&file.url)?;
        if response.body.len() > MAX_FILE_BYTES {
            return Err(catalog_err(
                index,
                format!(
                    "{} {}: {} bytes, the limit is {MAX_FILE_BYTES}",
                    entry.name,
                    file.path,
                    response.body.len()
                ),
            ));
        }
        let found = sha256_hex(&response.body);
        if found != file.sha256 {
            return Err(catalog_err(
                index,
                format!(
                    "{} {}: hash mismatch (expected {}…, downloaded {}…) — nothing was written",
                    entry.name,
                    file.path,
                    short_hash(&file.sha256),
                    short_hash(&found)
                ),
            ));
        }
        out.push((file.clone(), response.body));
    }
    Ok(out)
}

/// Whether the downloaded `plugin.json` is the manifest the index showed: the
/// permissions shown in the catalog must be the installed plugin's
/// permissions.
fn verify_manifest(
    index: &str,
    entry: &CatalogEntry,
    expected: &PluginManifest,
    files: &[(CatalogFile, Vec<u8>)],
) -> Result<()> {
    let Some((file, bytes)) = files.iter().find(|(file, _)| file.path == MANIFEST_FILE) else {
        return Err(catalog_err(
            index,
            format!("{}: `{MANIFEST_FILE}` was not downloaded", entry.name),
        ));
    };
    let raw = std::str::from_utf8(bytes).map_err(|err| {
        catalog_err(
            index,
            format!("{} {MANIFEST_FILE} is not UTF-8: {err}", entry.name),
        )
    })?;
    let downloaded = PluginManifest::parse(raw, &entry.name, Path::new(&file.url))
        .map_err(|err| catalog_err(index, cause_text(&err)))?;
    if downloaded != *expected {
        return Err(catalog_err(
            index,
            format!(
                "{}: the downloaded {MANIFEST_FILE} differs from what the index showed — the permissions \
                 shown in the catalog must be the installed plugin's; not installed",
                entry.name
            ),
        ));
    }
    Ok(())
}

/// Changes to the tools the engine installs, per this platform's binary.
fn tool_changes(old: &[Requirement], new: &[Requirement], platform: &str) -> Vec<ToolChange> {
    let names: BTreeSet<&str> = old
        .iter()
        .chain(new)
        .map(|requirement| requirement.name.as_str())
        .collect();
    let asset = |requirement: Option<&Requirement>| {
        requirement
            .and_then(|requirement| requirement.asset_for(platform))
            .map(|asset| (asset.url.clone(), asset.sha256.trim().to_ascii_lowercase()))
    };
    names
        .into_iter()
        .filter_map(|name| {
            let before = old.iter().find(|requirement| requirement.name == name);
            let after = new.iter().find(|requirement| requirement.name == name);
            let binary_changed = asset(before) != asset(after);
            let from = before.map(|requirement| requirement.version.clone());
            let to = after.map(|requirement| requirement.version.clone());
            (binary_changed || from != to).then(|| ToolChange {
                name: name.to_owned(),
                from,
                to,
                binary_changed,
            })
        })
        .collect()
}

fn record_files(files: &[(CatalogFile, Vec<u8>)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(file, _)| (file.path.clone(), file.sha256.clone()))
        .collect()
}

/// Joins a `/`-separated relative path to a directory — with each platform's
/// own separator.
fn relative(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_path_buf(), |acc, segment| acc.join(segment))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_err(Stage::PluginCatalog, parent, err))?;
    }
    std::fs::write(path, bytes).map_err(|err| io_err(Stage::PluginCatalog, path, err))
}

/// Replaces a file atomically: writes it next to the target under a
/// run-specific temporary name (D-060), then moves it over.
fn replace_file(dir: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let target = relative(dir, path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_err(Stage::PluginCatalog, parent, err))?;
    }
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(
        "{file_name}.{}-{}.downloading",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    ));
    write_file(&temp, bytes)?;
    if let Err(err) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(io_err(Stage::PluginCatalog, &target, err));
    }
    Ok(())
}

fn write_record(dir: &Path, record: &InstallRecord) -> Result<()> {
    let path = dir.join(ORIGIN_FILE);
    let mut text = serde_json::to_string_pretty(record).map_err(|source| {
        Error::new(
            Stage::PluginCatalog,
            ErrorKind::Json {
                entry: path.display().to_string(),
                source,
            },
        )
    })?;
    text.push('\n');
    replace_file(dir, ORIGIN_FILE, text.as_bytes())
}

/// The temporary directory an install is prepared in: in the data directory,
/// **outside** the plugin directory — so discovery does not mistake it for a
/// half-installed plugin while it is being prepared. If dropped before being
/// moved (error, panic), it deletes itself.
struct Staging {
    path: PathBuf,
    moved: bool,
}

impl Staging {
    fn create(data_dir: &Path, name: &str) -> Result<Self> {
        let path = data_dir.join(format!(
            ".plugin-{name}-{}-{}.installing",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ));
        std::fs::create_dir_all(&path).map_err(|err| io_err(Stage::PluginCatalog, &path, err))?;
        Ok(Self { path, moved: false })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Moves the ready directory into place. If the target appeared in the
    /// meantime (two installs at once), it is not overwritten, and this is said.
    fn move_to(mut self, target: &Path, index: &str) -> Result<()> {
        if std::fs::symlink_metadata(target).is_ok() {
            return Err(catalog_err(
                index,
                format!(
                    "another install got in while {} was being installed; not overwritten",
                    target.display()
                ),
            ));
        }
        std::fs::rename(&self.path, target)
            .map_err(|err| io_err(Stage::PluginCatalog, target, err))?;
        self.moved = true;
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.moved {
            return;
        }
        if let Err(err) = std::fs::remove_dir_all(&self.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.path.display(), error = %err, "could not delete the temporary install directory");
        }
    }
}

/// Generates the index from the plugins in the catalog repository (D-071).
///
/// Every subdirectory is a plugin: `<name>/plugin.json` + script. Directories
/// starting with a dot (`.git`, `.github`) and those without a `plugin.json`
/// are skipped. The manifest goes through **the core's own** validation — the
/// same rule installation applies; only plugins this version can install go
/// into the index.
///
/// `url_template` produces every file's address: `{name}`, `{version}` and
/// `{path}` are mandatory. `{version}` is mandatory because the address must
/// be pinned to the version (module docs, "Trust").
///
/// The output is deterministic: the same directory always produces the same
/// text, so that when `--check` sees a difference something really changed.
///
/// # Errors
/// If the template is invalid, the directory cannot be read, there are no
/// plugins, or **any** plugin is invalid — all of them at once, each with its
/// reason.
pub fn build_index(dir: &Path, url_template: &str) -> Result<BuiltIndex> {
    let origin = dir.display().to_string();
    validate_template(url_template).map_err(|detail| catalog_err(&origin, detail))?;

    let entries = std::fs::read_dir(dir).map_err(|err| io_err(Stage::PluginCatalog, dir, err))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| io_err(Stage::PluginCatalog, dir, err))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with('.') || !path.is_dir() || !path.join(MANIFEST_FILE).exists() {
            continue;
        }
        candidates.push((name, path));
    }
    candidates.sort();
    if candidates.is_empty() {
        return Err(catalog_err(
            &origin,
            format!(
                "no plugins in the directory — each plugin must live at `<name>/{MANIFEST_FILE}`; publishing \
                 an empty index would wipe the catalog"
            ),
        ));
    }

    let mut out = Vec::new();
    let mut plugins = Vec::new();
    let mut problems = Vec::new();
    for (name, path) in &candidates {
        match index_one(name, path, url_template) {
            Ok((entry, indexed)) => {
                out.push(entry);
                plugins.push(indexed);
            }
            Err(problem) => problems.push(format!("{name}: {problem}")),
        }
    }
    if !problems.is_empty() {
        return Err(catalog_err(
            &origin,
            format!(
                "{} plugins could not go into the index:\n  {}",
                problems.len(),
                problems.join("\n  ")
            ),
        ));
    }

    let mut json = serde_json::to_string_pretty(&IndexOut {
        schema: INDEX_SCHEMA,
        url_template,
        plugins: out,
    })
    .map_err(|err| catalog_err(&origin, format!("could not write the index: {err}")))?;
    json.push('\n');
    Ok(BuiltIndex { json, plugins })
}

fn index_one(
    name: &str,
    dir: &Path,
    url_template: &str,
) -> std::result::Result<(EntryOut, IndexedPlugin), String> {
    validate_catalog_name(name)?;
    let manifest = PluginManifest::load(dir).map_err(|err| cause_text(&err))?;
    let version = manifest
        .version
        .clone()
        .ok_or_else(|| "no `version` — every plugin in the catalog must be versioned".to_owned())?;
    validate_version(&version)?;

    let manifest_path = dir.join(MANIFEST_FILE);
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("{}: {err}", manifest_path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| format!("{}: {err}", manifest_path.display()))?;

    let mut files = Vec::new();
    for path in [MANIFEST_FILE.to_owned(), manifest.main_file()] {
        validate_file_path(&path)?;
        let local = relative(dir, &path);
        let bytes = std::fs::read(&local).map_err(|err| format!("{}: {err}", local.display()))?;
        files.push(CatalogFile {
            url: url_template
                .replace("{name}", name)
                .replace("{version}", &version)
                .replace("{path}", &path),
            path,
            sha256: sha256_hex(&bytes),
        });
    }
    Ok((
        EntryOut {
            manifest: value,
            files: files.clone(),
        },
        IndexedPlugin {
            name: name.to_owned(),
            version,
            files,
        },
    ))
}

fn validate_template(template: &str) -> std::result::Result<(), String> {
    for placeholder in ["{name}", "{version}", "{path}"] {
        if !template.contains(placeholder) {
            return Err(format!(
                "the address template has no `{placeholder}` — every file of every version must go to its \
                 own address (template: {template})"
            ));
        }
    }
    let sample = template
        .replace("{name}", "example")
        .replace("{version}", "1.0.0")
        .replace("{path}", "main.js");
    check_url(&sample, true).map_err(|detail| format!("address template: {detail}"))
}

/// Writes the index atomically to the catalog repository's root
/// ([`INDEX_FILE`]).
///
/// # Errors
/// If the file cannot be written.
pub fn write_index(dir: &Path, json: &str) -> Result<PathBuf> {
    replace_file(dir, INDEX_FILE, json.as_bytes())?;
    Ok(dir.join(INDEX_FILE))
}

/// The address template of an existing index — `plugin index` uses it when no
/// template is given, so that every build writes the same addresses. `None`
/// if the file does not exist.
///
/// # Errors
/// If the file exists but cannot be read or is not JSON.
pub fn read_url_template(index_path: &Path) -> Result<Option<String>> {
    let raw = match std::fs::read_to_string(index_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(io_err(Stage::PluginCatalog, index_path, err)),
    };
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|source| {
        Error::new(
            Stage::PluginCatalog,
            ErrorKind::Json {
                entry: index_path.display().to_string(),
                source,
            },
        )
    })?;
    Ok(value
        .get("url_template")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}

/// Which plugins differ between two index texts — so that when `--check` says
/// "out of date" it says **what** is out of date (K9).
#[must_use]
pub fn index_differences(existing: &str, built: &str) -> Vec<String> {
    fn by_name(text: &str) -> Option<BTreeMap<String, serde_json::Value>> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let plugins = value.get("plugins")?.as_array()?;
        Some(
            plugins
                .iter()
                .map(|entry| {
                    let name = entry
                        .get("manifest")
                        .and_then(|manifest| manifest.get("name"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    (name, entry.clone())
                })
                .collect(),
        )
    }
    let (Some(old), Some(new)) = (by_name(existing), by_name(built)) else {
        return vec!["the existing index.json could not be read".to_owned()];
    };
    let mut differences = Vec::new();
    for (name, entry) in &new {
        match old.get(name) {
            None => differences.push(format!("{name}: not in the index")),
            Some(previous) if previous != entry => {
                differences.push(format!("{name}: its entry changed"));
            }
            Some(_) => {}
        }
    }
    for name in old.keys() {
        if !new.contains_key(name) {
            differences.push(format!("{name}: in the index but has no directory"));
        }
    }
    if differences.is_empty() {
        differences.push(
            "the plugins are the same; the rest of the index (schema, template, format) differs"
                .to_owned(),
        );
    }
    differences
}

#[cfg(test)]
mod tests;
