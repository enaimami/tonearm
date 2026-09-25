//! The torrent provider plugin (protocol api 1) — Phase 2 §2.4, D-047.
//!
//! It runs not inside the core but **as a subprocess** (K5). The reason was
//! measured: `librqbit` added 179 crates to `headshell-core`'s dependency tree
//! (77 → 256), and that tree would go to mobile with `uniffi`. From here it
//! does not.
//!
//! ## A two-step search
//!
//! Torznab returns a **release**, not a track — usually an album. `WireTrack`,
//! on the other hand, is a track. Without growing api 1, the solution is two
//! steps:
//!
//! 1. `search "<query>"` → releases; each one's id is `<infohash>`.
//! 2. `search "<infohash>"` → that torrent's audio files; the ids are
//!    `<infohash>/<file index>`.
//!
//! `resolve_source` accepts both. For a release with a single audio file a
//! bare infohash plays directly; if there are several files it **does not
//! guess**, it returns an error that says what to do (K9).
//!
//! ## How the audio is delivered
//!
//! `resolve_source` returns the address of our own HTTP server listening on
//! `127.0.0.1` as an `HttpStream` (see [`stream`]). It does not wait for the
//! download to finish: `librqbit` sets piece priority by the read position.
//!
//! ## TODO: AFTER FIRST RELEASE — distribution breaks D-049
//!
//! This plugin makes the user run `cargo build --release -p
//! headshell-plugin-torrent`, that is, **it makes them install a Rust
//! toolchain.** D-049 requires that no plugin asks for a system-wide install,
//! and of the four plugins in the repository, after D-055 this is the **only**
//! one that breaks it: the others are scripts and go through the engine's
//! Python; this one is a binary and cannot.
//!
//! At one point the solution was "move it into the core as a provider behind a
//! feature" (D-050 S3). **D-056 cancelled that decision**: what would move is
//! 2,335 lines of source + 647 lines of tests, a working plugin — ripping it
//! out before the first release is not worth it.
//!
//! The open question is **distribution**, not architecture: will there be
//! prebuilt release artifacts per platform, or will it stay "build from
//! source". It will be decided after the first release — PLAN §2.8 item 5.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod engine;
pub mod release;
pub mod rpc;
pub mod stream;
pub mod torznab;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::OnceCell;

use headshell_core::plugin::protocol::{
    HandshakeParams, HealthResult, PLUGIN_API, ResolveSourceParams, SearchParams, SearchResult,
    WireTrack, method,
};
use headshell_core::provider::AudioSource;

use crate::engine::{CatalogEntry, Engine};
use crate::rpc::{Result, err};
use crate::stream::StreamServer;

pub const PLUGIN_NAME: &str = "torrent";
const DISPLAY_NAME: &str = "Torrent";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The keys in the secret namespace (D-042). Their values are printed nowhere.
const SECRET_TORZNAB_URL: &str = "torznab_url";
const SECRET_TORZNAB_KEY: &str = "torznab_api_key";

/// The answer `search` gives while Torznab is not configured.
///
/// **Not** an empty set: "I did not look" and "I did not find" are separate
/// diagnoses (K9).
const TORZNAB_MISSING: &str = concat!(
    "Torznab is not configured — search cannot be done (downloading and playback work). ",
    "Install Prowlarr or Jackett and give these: ",
    "`headshell secret set plugin:torrent torznab_url` ",
    "(e.g. http://127.0.0.1:9696/1/api) and ",
    "`headshell secret set plugin:torrent torznab_api_key`."
);

pub struct App {
    secrets: BTreeMap<String, String>,
    data_dir: PathBuf,
    engine: OnceCell<Arc<Engine>>,
    server: OnceCell<Arc<StreamServer>>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        Self {
            secrets: BTreeMap::new(),
            data_dir: PathBuf::from("."),
            engine: OnceCell::new(),
            server: OnceCell::new(),
        }
    }

    pub fn torznab(&self) -> Result<torznab::Torznab> {
        let Some(url) = self.secrets.get(SECRET_TORZNAB_URL).map(String::as_str) else {
            return err(TORZNAB_MISSING);
        };
        torznab::Torznab::new(
            url,
            self.secrets
                .get(SECRET_TORZNAB_KEY)
                .map(String::as_str)
                .unwrap_or_default(),
        )
    }

    /// The engine is set up the first time it is needed — not in the handshake.
    ///
    /// The handshake's timeout is 5 s, and opening a torrent session (binding a
    /// port, DHT bootstrap) can take longer. Setting it up there would make the
    /// plugin **unloadable** while the network is slow.
    pub async fn engine(&self) -> Result<Arc<Engine>> {
        self.engine
            .get_or_try_init(|| async { Engine::new(self.data_dir.clone()).await.map(Arc::new) })
            .await
            .cloned()
    }

    pub async fn server(&self) -> Result<Arc<StreamServer>> {
        let engine = self.engine().await?;
        self.server
            .get_or_try_init(|| async { StreamServer::spawn(engine).await })
            .await
            .cloned()
    }
}

pub async fn dispatch(
    app: &mut App,
    method_name: &str,
    params: serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    let value = match method_name {
        method::HANDSHAKE => handshake(app, params)?,
        method::HEALTH => health(app).await?,
        method::SEARCH => search(app, params).await?,
        method::RESOLVE_SOURCE => resolve_source(app, params).await?,
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub fn handshake(app: &mut App, params: serde_json::Value) -> Result<serde_json::Value> {
    let params: HandshakeParams = serde_json::from_value(params)
        .map_err(|error| rpc::PluginError::new(format!("could not read the handshake body: {error}")))?;
    if params.api != PLUGIN_API {
        return err(format!(
            "protocol version mismatch: core {}, plugin {PLUGIN_API}",
            params.api
        ));
    }
    app.secrets = params.secrets;
    app.data_dir = PathBuf::from(params.data_dir);

    to_value(&serde_json::json!({
        "api": PLUGIN_API,
        "name": PLUGIN_NAME,
        "display_name": DISPLAY_NAME,
        "plugin_version": VERSION,
        "capabilities": ["search", "stream"],
    }))
}

/// Health: **two separate** things are measured and reported separately.
///
/// Not reaching Torznab means search does not work; the torrent engine may
/// still be up, and a track whose infohash is at hand still plays. Reducing
/// the two to a single `reachable` flag makes the user fix the wrong thing
/// (K9).
pub async fn health(app: &App) -> Result<serde_json::Value> {
    let mut notes = Vec::new();
    let mut reachable = true;

    match app.torznab() {
        Ok(client) => match client.caps().await {
            Ok(detail) => notes.push(format!("search: {detail}")),
            Err(error) => {
                reachable = false;
                notes.push(format!("search does not work: {error}"));
            }
        },
        Err(error) => {
            reachable = false;
            notes.push(format!("search is not configured: {error}"));
        }
    }

    match app.engine().await {
        Ok(engine) => notes.push(format!(
            "the torrent engine is ready, download directory {}",
            engine.download_dir().display()
        )),
        Err(error) => {
            reachable = false;
            notes.push(format!("could not open the torrent engine: {error}"));
        }
    }

    let result = HealthResult {
        reachable,
        // A torrent has no catalog: giving a number would be making it up.
        track_count: None,
        detail: Some(notes.join(" | ")),
    };
    to_value(&result)
}

pub async fn search(app: &App, params: serde_json::Value) -> Result<serde_json::Value> {
    let params: SearchParams = serde_json::from_value(params)
        .map_err(|error| rpc::PluginError::new(format!("could not read the search body: {error}")))?;
    let query = params.query.trim();
    if query.is_empty() {
        return to_value(&SearchResult { tracks: Vec::new() });
    }

    // The second step: if the query is an infohash, the files inside that
    // torrent.
    let candidate = query.to_ascii_lowercase();
    if torznab::is_infohash(&candidate) {
        return search_inside(app, &candidate).await;
    }
    if let Some(hash) = torznab::infohash_from_magnet(query) {
        // A user pasting a magnet: we write it to the catalog and look inside.
        let engine = app.engine().await?;
        engine
            .remember(vec![(
                hash.clone(),
                CatalogEntry {
                    title: hash.clone(),
                    source_url: query.to_owned(),
                    indexer: None,
                },
            )])
            .await;
        return search_inside(app, &hash).await;
    }

    search_releases(app, query, params.limit).await
}

/// The first step: search Torznab for releases.
pub async fn search_releases(app: &App, query: &str, limit: usize) -> Result<serde_json::Value> {
    let client = app.torznab()?;
    let outcome = client.search(query, limit).await?;

    if outcome.dropped_unidentifiable > 0 {
        rpc::log(
            "warn",
            format!(
                "{} results were dropped because they carry no infohash (they could not be played)",
                outcome.dropped_unidentifiable
            ),
        );
    }

    let engine = app.engine().await?;
    let mut remember = Vec::new();
    let mut tracks = Vec::new();
    for found in &outcome.releases {
        let Some(source_url) = found.source_url() else {
            continue;
        };
        remember.push((
            found.infohash.clone(),
            CatalogEntry {
                title: found.title.clone(),
                source_url: source_url.to_owned(),
                indexer: found.indexer.clone(),
            },
        ));
        let parsed = release::parse_release_name(&found.title);
        tracks.push(WireTrack {
            id: found.infohash.clone(),
            artist: parsed.artist,
            // The release name is the album; we put it into the title field too,
            // because this row is not a track but a release. It is opened with
            // `search "<infohash>"`.
            title: parsed.title.clone(),
            album: Some(parsed.title),
            // A release has no duration. Making one up would misdirect the
            // identity chain's duration tie-breaker (D-046 addendum 2).
            duration_ms: None,
            isrc: None,
        });
    }
    engine.remember(remember).await;

    to_value(&SearchResult { tracks })
}

/// The second step: the audio files inside a torrent.
pub async fn search_inside(app: &App, infohash: &str) -> Result<serde_json::Value> {
    let engine = app.engine().await?;
    let (source_url, from_catalog) = engine.source_for(infohash).await;
    if !from_catalog {
        rpc::log(
            "info",
            format!("{infohash} is not in the catalog; without a tracker list it will be looked for through the DHT only"),
        );
    }
    let handle = engine.handle(infohash, &source_url).await?;
    let files = Engine::audio_files(&handle)?;
    if files.is_empty() {
        return err(format!(
            "no audio files in the torrent ({infohash}); state: {}",
            Engine::progress(&handle)
        ));
    }

    let release_name = engine
        .lookup(infohash)
        .await
        .map(|entry| entry.title)
        .unwrap_or_default();
    let parsed = release::parse_release_name(&release_name);

    let tracks = files
        .iter()
        .map(|file| {
            let (title, _track_no) = release::parse_file_name(&file.file_name);
            WireTrack {
                id: format!("{infohash}/{}", file.index),
                artist: parsed.artist.clone(),
                title,
                album: (!parsed.title.is_empty()).then(|| parsed.title.clone()),
                // The duration is not in the torrent metadata — it cannot be known
                // without decoding the file. The identity chain learns it from the file
                // (the fingerprint route).
                duration_ms: None,
                isrc: None,
            }
        })
        .collect();

    to_value(&SearchResult { tracks })
}

pub async fn resolve_source(app: &App, params: serde_json::Value) -> Result<serde_json::Value> {
    let params: ResolveSourceParams = serde_json::from_value(params)
        .map_err(|error| rpc::PluginError::new(format!("could not read the resolve body: {error}")))?;
    let raw = params.id.trim();
    if raw.is_empty() {
        return err("the track id is empty");
    }

    let (infohash, wanted_index) = match raw.split_once('/') {
        Some((hash, index)) => {
            let parsed: usize = index
                .parse()
                .map_err(|_| rpc::PluginError::new(format!("the file index is not a number: {index}")))?;
            (hash.to_ascii_lowercase(), Some(parsed))
        }
        None => (raw.to_ascii_lowercase(), None),
    };
    if !torznab::is_infohash(&infohash) {
        return err(format!("the id is not an infohash: {infohash}"));
    }

    let engine = app.engine().await?;
    let (source_url, _) = engine.source_for(&infohash).await;
    let handle = engine.handle(&infohash, &source_url).await?;
    let files = Engine::audio_files(&handle)?;

    let index = match wanted_index {
        Some(index) => {
            if !files.iter().any(|file| file.index == index) {
                // "None" is an answer, not an error (protocol §resolve_source).
                return to_value(&headshell_core::plugin::protocol::ResolveSourceResult {
                    source: None,
                });
            }
            index
        }
        None => match files.as_slice() {
            [] => {
                return err(format!(
                    "no audio files in the torrent ({infohash}); state: {}",
                    Engine::progress(&handle)
                ));
            }
            [single] => single.index,
            many => {
                // We do not know which one it is, and we **do not guess** (K9):
                // picking the first file would silently play the wrong track.
                let listing = many
                    .iter()
                    .take(10)
                    .map(|file| format!("{}: {}", file.index, file.file_name))
                    .collect::<Vec<_>>()
                    .join(", ");
                let extra = if many.len() > 10 {
                    format!(" (+{} more files)", many.len() - 10)
                } else {
                    String::new()
                };
                return err(format!(
                    "this release has {} audio files, and you did not say which one. \
                     `headshell provider search torrent {infohash}` lists the files; \
                     the id becomes `{infohash}/<index>`. Files — {listing}{extra}",
                    many.len()
                ));
            }
        },
    };

    let server = app.server().await?;
    let url = server.url_for(&infohash, index);
    rpc::log(
        "info",
        format!(
            "{infohash}/{index} is getting ready to stream: {}",
            Engine::progress(&handle)
        ),
    );

    to_value(&headshell_core::plugin::protocol::ResolveSourceResult {
        source: Some(AudioSource::HttpStream {
            url,
            headers: Vec::new(),
        }),
    })
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value)
        .map_err(|error| rpc::PluginError::new(format!("could not serialise the answer: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missing_torznab_message_tells_the_user_exactly_what_to_type() {
        assert!(TORZNAB_MISSING.contains("headshell secret set plugin:torrent torznab_url"));
        assert!(TORZNAB_MISSING.contains("torznab_api_key"));
        // "No search" must not be confused with "nothing works".
        assert!(TORZNAB_MISSING.contains("playback work"));
    }

    #[test]
    fn an_unconfigured_search_is_an_error_not_an_empty_result() {
        let app = App::new();
        let error = app.torznab().unwrap_err();
        assert!(error.to_string().contains("not configured"), "{error}");
    }

    #[test]
    fn the_handshake_answers_with_the_name_the_manifest_declares() {
        let mut app = App::new();
        let value = handshake(
            &mut app,
            serde_json::json!({
                "api": PLUGIN_API,
                "host": {"name": "headshell", "version": "0.0.1"},
                "data_dir": "/tmp/x",
                "secrets": {"torznab_url": "https://x/api"},
                "permissions": {"net": [], "fs": []},
            }),
        )
        .unwrap();
        assert_eq!(value["name"], PLUGIN_NAME);
        assert_eq!(value["api"], PLUGIN_API);
        assert_eq!(
            value["capabilities"],
            serde_json::json!(["search", "stream"])
        );
        assert_eq!(app.data_dir, PathBuf::from("/tmp/x"));
        assert!(app.torznab().is_ok(), "the secret must be taken from the handshake");
    }

    #[test]
    fn a_version_mismatch_is_refused_with_both_numbers_visible() {
        let mut app = App::new();
        let error = handshake(
            &mut app,
            serde_json::json!({
                "api": PLUGIN_API + 7,
                "host": {"name": "headshell", "version": "0.0.1"},
                "data_dir": "/tmp/x",
                "secrets": {},
                "permissions": {"net": [], "fs": []},
            }),
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains(&(PLUGIN_API + 7).to_string()), "{text}");
        assert!(text.contains(&PLUGIN_API.to_string()), "{text}");
    }

    #[tokio::test]
    async fn an_empty_query_is_an_empty_result_without_touching_the_network() {
        let app = App::new();
        // Torznab is not configured; still no error, because an empty query must
        // never go to the index.
        let value = search(&app, serde_json::json!({"query": "  ", "limit": 10}))
            .await
            .unwrap();
        assert_eq!(value["tracks"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn an_id_that_is_not_an_infohash_says_so_instead_of_starting_a_download() {
        let app = App::new();
        let error = resolve_source(&app, serde_json::json!({"id": "hello"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not an infohash"), "{error}");
    }

    #[tokio::test]
    async fn a_file_index_that_is_not_a_number_is_refused_before_any_work() {
        let app = App::new();
        let hash = "e".repeat(40);
        let error = resolve_source(&app, serde_json::json!({"id": format!("{hash}/abc")}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not a number"), "{error}");
    }

    #[tokio::test]
    async fn an_unknown_method_is_method_not_found_not_a_crash() {
        let mut app = App::new();
        let outcome = dispatch(&mut app, "teleport", serde_json::Value::Null)
            .await
            .unwrap();
        assert!(outcome.is_none());
    }
}
