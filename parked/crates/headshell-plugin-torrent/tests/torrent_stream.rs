//! End to end: a real HTTP stream from a real torrent (D-047).
//!
//! ## Why there are no peers
//!
//! The test **creates** a torrent and puts its data into the download
//! directory beforehand; `librqbit` does a hash check at start-up and counts
//! the torrent as complete. That way what is tested is **our** code —
//! extracting the file list from the metadata, opening the stream, answering
//! `Range` — not `librqbit`'s ability to find peers. A test depending on peers
//! would be a test that sometimes passes depending on the state of the
//! network, and it would mix up the two failures D-043 wants to keep apart.
//!
//! What is not tested, explicitly: **downloading** from peers. That is the
//! job of `librqbit`'s own test suite, not a line we wrote.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use headshell_plugin_torrent::engine::Engine;
use headshell_plugin_torrent::stream::StreamServer;
use librqbit::CreateTorrentOptions;
use librqbit::spawn_utils::BlockingSpawner;

/// The audio fixture in the repository (made in D-046; synthetic, not a
/// copyrighted recording).
const FIXTURE: &str = "../../fixtures/audio/fingerprint_sample.flac";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "headshell-torrent-it-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    std::fs::create_dir_all(&dir).expect("temporary directory");
    dir
}

/// Creates a torrent, places its data in the download directory and returns
/// the engine.
async fn seeded_engine(name: &str) -> (Arc<Engine>, String, Vec<u8>, PathBuf) {
    let root = scratch(name);
    let data_dir = root.join("state");
    std::fs::create_dir_all(&data_dir).expect("state directory");

    let content = std::fs::read(Path::new(FIXTURE)).expect("the fixture must be readable");
    let staging = root.join("staging");
    std::fs::create_dir_all(&staging).expect("staging directory");
    let staged_file = staging.join("sample.flac");
    std::fs::write(&staged_file, &content).expect("the fixture must be copied");

    let created = librqbit::create_torrent(
        &staged_file,
        CreateTorrentOptions {
            name: None,
            trackers: Vec::new(),
            piece_length: Some(16 * 1024),
        },
        &BlockingSpawner::new(4),
    )
    .await
    .expect("the torrent must be created");

    let infohash = created.info_hash().as_string();
    let torrent_path = root.join("sample.torrent");
    std::fs::write(&torrent_path, created.as_bytes().expect("torrent bytes"))
        .expect("the torrent file must be written");

    // Put the data where the engine will look beforehand:
    // `<download>/<infohash>/sample.flac`.
    let engine = Arc::new(Engine::new(data_dir).await.expect("the engine must open"));
    let output = engine.download_dir().join(&infohash);
    std::fs::create_dir_all(&output).expect("output directory");
    std::fs::write(output.join("sample.flac"), &content).expect("the data must be placed");

    // The same as the real flow: the source seen in a search is written to the
    // catalog, and playback reads it from there. If we did not write it, the
    // stream server would produce a bare magnet and look for peers — there are
    // no peers at all in this test.
    engine
        .remember(vec![(
            infohash.clone(),
            headshell_plugin_torrent::engine::CatalogEntry {
                title: "Test Release".to_owned(),
                source_url: torrent_path.display().to_string(),
                indexer: None,
            },
        )])
        .await;

    (engine, infohash, content, torrent_path)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_torrents_audio_files_are_listed_from_its_metadata() {
    let (engine, infohash, content, torrent_path) = seeded_engine("list").await;
    let handle = engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("the torrent must be ready");

    let files = Engine::audio_files(&handle).expect("the files must be listed");
    assert_eq!(files.len(), 1, "a single audio file is expected: {files:?}");
    assert_eq!(files[0].index, 0);
    assert_eq!(files[0].file_name, "sample.flac");
    assert_eq!(
        files[0].len,
        content.len() as u64,
        "the length in the metadata must match the real file"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_local_stream_serves_the_whole_file_byte_for_byte() {
    let (engine, infohash, content, torrent_path) = seeded_engine("tam").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("the torrent must be ready");

    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("stream server");
    let url = server.url_for(&infohash, 0);
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "local only: {url}"
    );

    let response = reqwest::get(&url).await.expect("the stream must be requestable");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("audio/flac")
    );
    assert_eq!(
        response
            .headers()
            .get("accept-ranges")
            .and_then(|value| value.to_str().ok()),
        Some("bytes"),
        "the player must be able to seek"
    );
    let body = response.bytes().await.expect("body").to_vec();
    assert_eq!(body.len(), content.len());
    assert_eq!(body, content, "the streamed bytes must be the file itself");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_request_returns_exactly_that_slice() {
    let (engine, infohash, content, torrent_path) = seeded_engine("range").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("the torrent must be ready");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("stream server");

    let client = reqwest::Client::new();
    let response = client
        .get(server.url_for(&infohash, 0))
        // A slice past a piece boundary: does seeking in `FileStream` really work,
        // or does it only read from the start?
        .header("Range", "bytes=20000-20099")
        .send()
        .await
        .expect("the range must be requestable");

    assert_eq!(response.status().as_u16(), 206);
    assert_eq!(
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok()),
        Some(format!("bytes 20000-20099/{}", content.len()).as_str())
    );
    let body = response.bytes().await.expect("body").to_vec();
    assert_eq!(body.len(), 100);
    assert_eq!(
        body,
        content[20000..20100],
        "the slice must come from exactly that place in the file"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_range_past_the_end_is_416_not_a_silent_restart_from_zero() {
    let (engine, infohash, content, torrent_path) = seeded_engine("416").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("the torrent must be ready");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("stream server");

    let response = reqwest::Client::new()
        .get(server.url_for(&infohash, 0))
        .header("Range", format!("bytes={}-", content.len() + 1000))
        .send()
        .await
        .expect("the request must be sendable");
    assert_eq!(
        response.status().as_u16(),
        416,
        "silently sending from the start for a range outside the file takes the player to the wrong position"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_token_is_not_decoration_a_wrong_one_is_refused() {
    let (engine, infohash, _content, torrent_path) = seeded_engine("token").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("the torrent must be ready");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("stream server");

    let real = server.url_for(&infohash, 0);
    // We break the token; the rest of the address is right.
    let forged = real.replacen(
        real.split('/').nth(3).expect("the token part"),
        "0000000000000000000000000000000",
        1,
    );
    let response = reqwest::get(&forged).await.expect("the request must be sendable");
    assert_eq!(
        response.status().as_u16(),
        404,
        "the downloads must not be readable without the token"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_index_that_does_not_exist_is_404_not_a_hang() {
    let (engine, infohash, _content, torrent_path) = seeded_engine("missing").await;
    engine
        .handle(&infohash, &torrent_path.display().to_string())
        .await
        .expect("the torrent must be ready");
    let server = StreamServer::spawn(Arc::clone(&engine))
        .await
        .expect("stream server");

    let response = reqwest::get(server.url_for(&infohash, 99))
        .await
        .expect("the request must be sendable");
    assert_eq!(response.status().as_u16(), 404);
}

/// A source without peers must give up **within the budget**.
///
/// The reason for this test is a flaw: the budget first only wrapped
/// `wait_until_initialized`, when for a magnet it is `add_torrent` itself that
/// resolves the metadata. A cold magnet kept the plugin waiting forever, the
/// core said "the plugin hung" after 20 s, and the user never learned why.
/// Only a real run showed the flaw — a fake session would have said "it
/// returned right away".
#[tokio::test(flavor = "multi_thread")]
async fn a_source_with_no_peers_gives_up_within_the_budget_and_says_why() {
    let root = scratch("no-peers");
    let data_dir = root.join("state");
    std::fs::create_dir_all(&data_dir).expect("state directory");
    let engine = Engine::new(data_dir).await.expect("the engine must open");

    // An infohash that is nowhere: not in the catalog, no tracker, not in the
    // DHT.
    let nowhere = "f".repeat(40);
    let (source, from_catalog) = engine.source_for(&nowhere).await;
    assert!(!from_catalog, "the catalog must be empty");

    let started = std::time::Instant::now();
    let outcome = engine.handle(&nowhere, &source).await;
    let elapsed = started.elapsed();
    let Err(error) = outcome else {
        panic!("no success must come back for a torrent that is nowhere");
    };

    assert!(
        elapsed < headshell_core::plugin::client::CALL_TIMEOUT,
        "we should have answered before the core's timeout: {elapsed:?}"
    );
    let text = error.to_string();
    assert!(
        text.contains("no peers"),
        "the reason must be given: {text}"
    );
    assert!(
        text.contains("try again"),
        "what to do must be said: {text}"
    );
}
