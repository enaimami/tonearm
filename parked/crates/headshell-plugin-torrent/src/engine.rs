//! The torrent engine: the `librqbit` session, the catalog and the file list.
//!
//! ## Why there is a catalog file
//!
//! `search` returns a magnet; `resolve_source` may arrive in **another run of
//! the process** (the core closes the plugin and opens it again). If only the
//! infohash is left we lose the tracker list, and finding the torrent is left
//! to the DHT — sometimes minutes, sometimes never. That is why the magnet of
//! every release seen in a search is written to `catalog.json`.
//!
//! ## Why every call has a budget
//!
//! The core's call timeout is 20 s (`client::CALL_TIMEOUT`). Resolving a cold
//! magnet's metadata can take longer than that. Instead of a fixed `sleep` we
//! do a **real readiness check** (the bash prototype's lesson, PLAN §2.4), and
//! when the budget runs out we say "not ready yet, it carries on in the
//! background, try again" — not a silent empty result (K9).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::release;
use crate::rpc::{PluginError, Result, chain_text, err};

/// `librqbit`'s own `ManagedTorrentHandle` alias is not public; we keep the
/// same type here.
pub type TorrentHandle = Arc<ManagedTorrent>;

/// The time allowed for resolving the metadata. It must stay below the core's
/// 20 s call timeout, so that the timeout shows up on **our** side, as an
/// explained answer.
pub const METADATA_BUDGET: std::time::Duration = std::time::Duration::from_secs(14);

/// A single audio file inside a torrent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFile {
    /// The file's order inside the torrent. This is the second part of the id.
    pub index: usize,
    pub relative_path: String,
    pub file_name: String,
    pub len: u64,
}

/// A single record kept in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub title: String,
    /// A magnet or `.torrent` address — the form that carries the tracker list.
    pub source_url: String,
    #[serde(default)]
    pub indexer: Option<String>,
}

/// `catalog.json`. Small, human-readable, and if it is lost, only speed is
/// lost.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Catalog {
    #[serde(default)]
    entries: BTreeMap<String, CatalogEntry>,
}

pub struct Engine {
    session: Arc<Session>,
    download_dir: PathBuf,
    catalog_path: PathBuf,
    catalog: Mutex<Catalog>,
}

impl Engine {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        let download_dir = data_dir.join("downloads");
        std::fs::create_dir_all(&download_dir).map_err(|error| {
            PluginError::new(format!(
                "could not create the download directory ({}): {error}",
                download_dir.display()
            ))
        })?;

        let session = Session::new(download_dir.clone()).await.map_err(|error| {
            PluginError::new(format!("could not open the torrent session: {}", chain_text(&error)))
        })?;

        let catalog_path = data_dir.join("catalog.json");
        let catalog = read_catalog(&catalog_path);
        Ok(Self {
            session,
            download_dir,
            catalog_path,
            catalog: Mutex::new(catalog),
        })
    }

    pub fn download_dir(&self) -> &std::path::Path {
        &self.download_dir
    }

    /// Writes the releases seen in a search to the catalog. If it cannot be
    /// written, the work does not stop, but it **does not stay silent** — it means
    /// the next `resolve_source` will be slower.
    pub async fn remember(&self, entries: Vec<(String, CatalogEntry)>) {
        if entries.is_empty() {
            return;
        }
        let mut catalog = self.catalog.lock().await;
        for (infohash, entry) in entries {
            catalog.entries.insert(infohash, entry);
        }
        if let Err(error) = write_catalog(&self.catalog_path, &catalog) {
            crate::rpc::log(
                "warn",
                format!("could not write the catalog, the next play may be slower: {error}"),
            );
        }
    }

    pub async fn lookup(&self, infohash: &str) -> Option<CatalogEntry> {
        self.catalog.lock().await.entries.get(infohash).cloned()
    }

    /// Produces a source address for an infohash.
    ///
    /// A magnet with trackers if it is in the catalog, otherwise a bare magnet —
    /// the bare form relies on the DHT alone, and we say so, because that is
    /// often the reason behind a "no peers found" diagnosis.
    pub async fn source_for(&self, infohash: &str) -> (String, bool) {
        match self.lookup(infohash).await {
            Some(entry) => (entry.source_url, true),
            None => (format!("magnet:?xt=urn:btih:{infohash}"), false),
        }
    }

    /// Adds the torrent to the session (or gives its handle if already added) and
    /// waits, up to the budget, until its metadata is resolved.
    pub async fn handle(&self, infohash: &str, source_url: &str) -> Result<TorrentHandle> {
        let options = AddTorrentOptions {
            // Every torrent into its own directory (PLAN §2.4, the bash
            // prototype's lesson): two releases carrying the same file name is
            // common, and overwriting is silent data loss.
            output_folder: Some(self.download_dir.join(infohash).display().to_string()),
            overwrite: true,
            ..Default::default()
        };
        let add = add_torrent_from(source_url)?;
        // **The budget has to cover `add_torrent` too.** For a magnet, it is
        // not `wait_until_initialized` that resolves the metadata but
        // `add_torrent` itself: if no peer is found it waits there forever.
        // Counting only the wait made the plugin fall into the core's 20 s
        // timeout on a cold magnet — so the user saw "the plugin hung" and
        // never learned why (K9).
        let acquire = async {
            let response = self
                .session
                .add_torrent(add, Some(options))
                .await
                .map_err(|error| {
                    PluginError::new(format!(
                        "the torrent could not be added ({infohash}): {}",
                        chain_text(&error)
                    ))
                })?;
            let Some(handle) = response.into_handle() else {
                return err(format!(
                    "the torrent was only listed, it did not come to life ({infohash})"
                ));
            };
            handle.wait_until_initialized().await.map_err(|error| {
                PluginError::new(format!(
                    "could not resolve the torrent metadata ({infohash}): {}",
                    chain_text(&error)
                ))
            })?;
            Ok(handle)
        };

        match tokio::time::timeout(METADATA_BUDGET, acquire).await {
            Ok(result) => result,
            Err(_) => err(format!(
                "the torrent metadata did not arrive in {} s ({infohash}); no peers may have \
                 been found. If the source carries no trackers we only have the DHT — \
                 searching for the magnet (`headshell provider search torrent <magnet>`) \
                 writes the tracker list to the catalog. The download carries on in the \
                 background; try again.",
                METADATA_BUDGET.as_secs()
            )),
        }
    }

    /// The audio files inside the torrent, in their order in the torrent.
    pub fn audio_files(handle: &TorrentHandle) -> Result<Vec<AudioFile>> {
        let Some(metadata) = handle.metadata.load_full() else {
            return err("no torrent metadata yet");
        };
        let mut files = Vec::new();
        for (index, info) in metadata.file_infos.iter().enumerate() {
            if !release::is_audio(&info.relative_filename) {
                continue;
            }
            let file_name = info
                .relative_filename
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            files.push(AudioFile {
                index,
                relative_path: info.relative_filename.display().to_string(),
                file_name,
                len: info.len,
            });
        }
        Ok(files)
    }

    /// A human-readable progress summary — peer count and speed included (PLAN
    /// §2.4).
    pub fn progress(handle: &TorrentHandle) -> String {
        let stats = handle.stats();
        let mut text = format!("{} / {} bytes", stats.progress_bytes, stats.total_bytes);
        if let Some(live) = stats.live.as_ref() {
            text.push_str(&format!(
                ", {} peer, {}",
                live.snapshot.peer_stats.live, live.download_speed
            ));
        }
        if let Some(error) = stats.error.as_ref() {
            text.push_str(&format!(", error: {error}"));
        }
        text
    }
}

/// Turns the source into the form `librqbit` understands.
///
/// `magnet:` / `http(s):` go straight through; everything else is read as **a
/// local `.torrent` file**. That way a user who has a file (and the tests)
/// goes the same way. Taking something unrecognised for an address and
/// leaving it to `librqbit`'s "invalid URL" error would put the diagnosis in
/// the wrong place.
fn add_torrent_from(source: &str) -> Result<AddTorrent<'_>> {
    if librqbit::SUPPORTED_SCHEMES
        .iter()
        .any(|scheme| source.starts_with(scheme))
    {
        return Ok(AddTorrent::from_url(source));
    }
    let path = std::path::Path::new(source);
    if !path.is_file() {
        return err(format!(
            "the source is neither a magnet/http address nor an existing .torrent file: {source}"
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| PluginError::new(format!("could not read the `.torrent` ({source}): {error}")))?;
    Ok(AddTorrent::from_bytes(bytes))
}

fn read_catalog(path: &std::path::Path) -> Catalog {
    let Ok(raw) = std::fs::read_to_string(path) else {
        // If there is no file an empty catalog is the right answer; if it cannot be
        // read we carry on too, but we will notice on the first write.
        return Catalog::default();
    };
    match serde_json::from_str(&raw) {
        Ok(catalog) => catalog,
        Err(error) => {
            crate::rpc::log(
                "warn",
                format!("the catalog is corrupt, taken as empty ({}): {error}", path.display()),
            );
            Catalog::default()
        }
    }
}

fn write_catalog(path: &std::path::Path, catalog: &Catalog) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(catalog).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, raw)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "headshell-torrent-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_catalog_round_trips_through_the_file() {
        let dir = temp_dir("catalog");
        let path = dir.join("catalog.json");
        let mut catalog = Catalog::default();
        catalog.entries.insert(
            "a".repeat(40),
            CatalogEntry {
                title: "A Release".to_owned(),
                source_url: "magnet:?xt=urn:btih:x&tr=udp://t".to_owned(),
                indexer: Some("example".to_owned()),
            },
        );
        write_catalog(&path, &catalog).unwrap();

        let read = read_catalog(&path);
        assert_eq!(read.entries.len(), 1);
        assert_eq!(
            read.entries[&"a".repeat(40)].source_url,
            "magnet:?xt=urn:btih:x&tr=udp://t"
        );
    }

    #[test]
    fn a_missing_catalog_is_empty_not_an_error() {
        let dir = temp_dir("missing");
        assert!(read_catalog(&dir.join("catalog.json")).entries.is_empty());
    }

    #[test]
    fn a_corrupt_catalog_does_not_take_the_plugin_down() {
        let dir = temp_dir("broken");
        let path = dir.join("catalog.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(read_catalog(&path).entries.is_empty());
    }

    #[test]
    fn the_metadata_budget_stays_under_the_cores_call_timeout() {
        // If the budget exceeds the core's timeout, the error shows up not on
        // **our** side as an explained answer but in the core as "the plugin hung"
        // — and the user cannot learn the reason.
        assert!(
            METADATA_BUDGET < headshell_core::plugin::client::CALL_TIMEOUT,
            "budget {METADATA_BUDGET:?}, timeout {:?}",
            headshell_core::plugin::client::CALL_TIMEOUT
        );
    }
}
