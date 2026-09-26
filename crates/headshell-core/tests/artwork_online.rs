//! Covers, **against the real MusicBrainz and the real Cover Art Archive**
//! (D-076).
//!
//! The chain's unit tests run on canned answers: they test the logic. This
//! tests that the logic meets the data it was written for — a well-known
//! song, whose album is the answer and whose namesake recordings on
//! compilations are the trap. Verification found the trap by hand: "Sour
//! Times" has 77 recordings at MusicBrainz, and the identity chain matched
//! one on a compilation, whose cover came back for the album.
//!
//! **Part of the default run** (D-043), with the usual two failures kept
//! apart (K9):
//! - **Not reaching** either service is not a failure: the test skips itself
//!   and writes the reason to `stderr` — before it starts (no TCP) or on the
//!   way (a network error, an exhausted `503` retry).
//! - **Reaching them and getting the unexpected** fails: no cover, another
//!   album's cover, an error that is not the network's.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::path::Path;

use headshell_core::artwork::{ArtworkSource, ArtworkStatus};
use headshell_core::error::ErrorKind;
use headshell_core::provider::ProviderRegistry;
use headshell_core::session::{ArtworkRequest, LookupMode, Session};

/// Can `host` be reached over TCP. Only DNS + connect: "is there a
/// network", not the service's health — that is the test's job.
fn reachable(host: &str) -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = (host, 443).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).is_ok()
    })
}

/// The chain's failure text, if it failed for the network's reasons: then
/// the test skips.
fn network_failure(chain: &str) -> bool {
    chain.contains("STEP: NETWORK_REQUEST") || chain.contains("HTTP 503")
}

#[tokio::test]
async fn a_real_albums_cover_comes_from_the_archive_not_a_compilations() {
    const TEST: &str = "cover from the archive";
    for host in ["musicbrainz.org", "coverartarchive.org"] {
        if !reachable(host) {
            eprintln!("{TEST}: could not reach {host}:443 — skipped (taken as no network)");
            return;
        }
    }

    let config = support::TestConfig::new("artwork-online");
    let mut session = Session::open((*config).clone()).unwrap();
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/artwork/Portishead - Sour Times.flac");
    let request = ArtworkRequest {
        file: Some(file),
        ..ArtworkRequest::default()
    };
    let report = match session
        .artwork(&ProviderRegistry::new(), request, LookupMode::Online)
        .await
    {
        Ok(report) => report,
        Err(err) if matches!(err.kind(), ErrorKind::Network { .. }) => {
            eprintln!("{TEST}: {} — skipped (network)", err.chain_text());
            return;
        }
        Err(err) => panic!("{TEST}: {}", err.chain_text()),
    };

    let item = &report.items[0];
    match &item.status {
        ArtworkStatus::Found {
            source: ArtworkSource::CoverArtArchive,
        } => {}
        ArtworkStatus::Failed { chain } if network_failure(chain) => {
            eprintln!("{TEST}: {chain} — skipped (network or quota)");
            return;
        }
        other => panic!("{TEST}: expected a cover from the archive, got {other:?}\n{report:#?}"),
    }

    // Which release it came from is in the cache's index: it must be the
    // album's. A compilation's cover would be a wrong answer that looks right.
    let index = std::fs::read_to_string(config.artwork_dir().join("index.json")).unwrap();
    assert!(
        index.contains("(\\\"Dummy\\\")"),
        "{TEST}: the cover is not from a release titled \"Dummy\":\n{index}"
    );
}
