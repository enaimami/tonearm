//! The live Torznab test (under D-043's rule).
//!
//! Torznab has no public instance — the user's own Prowlarr or Jackett is
//! needed. So besides "cannot reach it" there is also a "not configured" state
//! here, and **neither is a failure**: the test skips itself and writes the
//! reason to `stderr`. If it reaches the indexer and gets the unexpected, it
//! fails.
//!
//! ```bash
//! HEADSHELL_TORZNAB_URL=http://127.0.0.1:9696/1/api \
//! HEADSHELL_TORZNAB_KEY=... cargo test -p headshell-plugin-torrent --test torznab_live
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use headshell_plugin_torrent::torznab::Torznab;

fn client_or_skip(test: &str) -> Option<Torznab> {
    let Ok(url) = std::env::var("HEADSHELL_TORZNAB_URL") else {
        eprintln!(
            "{test}: skipped — no HEADSHELL_TORZNAB_URL. \
             The live test needs the user's own Prowlarr/Jackett."
        );
        return None;
    };
    let key = std::env::var("HEADSHELL_TORZNAB_KEY").unwrap_or_default();
    match Torznab::new(&url, &key) {
        Ok(client) => Some(client),
        Err(error) => {
            // If the address is malformed, that is a configuration error and
            // not something to skip: the user asked for a live test.
            panic!("{test}: HEADSHELL_TORZNAB_URL is invalid: {error}");
        }
    }
}

/// Is it a network error, or an answer the indexer gave? They are separate
/// diagnoses (K9).
fn is_unreachable(message: &str) -> bool {
    message.contains("could not reach") || message.contains("could not read the Torznab answer")
}

#[tokio::test]
async fn the_indexer_answers_a_capabilities_request() {
    let Some(client) = client_or_skip("caps") else {
        return;
    };
    match client.caps().await {
        Ok(detail) => {
            assert!(detail.contains("categories"), "{detail}");
        }
        Err(error) => {
            let message = error.to_string();
            assert!(
                is_unreachable(&message),
                "the indexer answered but said something unexpected: {message}"
            );
            eprintln!("caps: skipped — could not reach the indexer: {message}");
        }
    }
}

#[tokio::test]
async fn a_real_search_returns_releases_that_carry_an_infohash() {
    let Some(client) = client_or_skip("search") else {
        return;
    };
    let query = std::env::var("HEADSHELL_TORZNAB_QUERY").unwrap_or_else(|_| "radiohead".to_owned());

    let outcome = match client.search(&query, 20).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let message = error.to_string();
            assert!(
                is_unreachable(&message),
                "the indexer answered but said something unexpected: {message}"
            );
            eprintln!("search: skipped — could not reach the indexer: {message}");
            return;
        }
    };

    // Zero results is **not** a failure: the user's indexer may have nothing
    // for that query. What is tested is the format, not the catalog.
    eprintln!(
        "search: {} releases, {} records dropped for having no id",
        outcome.releases.len(),
        outcome.dropped_unidentifiable
    );

    for release in &outcome.releases {
        assert_eq!(
            release.infohash.len(),
            40,
            "an infohash must be 40 digits: {release:?}"
        );
        assert!(
            release.source_url().is_some(),
            "a release that cannot be played must not get into the list: {release:?}"
        );
        assert!(!release.title.trim().is_empty(), "{release:?}");
    }
}

/// With a wrong key we must get **an error, not an empty result**.
///
/// This is the torrent-side counterpart of the lesson D-046 learned at
/// AcoustID: reading a refusal as "not found" makes the user fix the wrong
/// thing. On an install that needs no key it is skipped.
#[tokio::test]
async fn a_rejected_key_is_an_error_not_an_empty_result() {
    let Ok(url) = std::env::var("HEADSHELL_TORZNAB_URL") else {
        eprintln!("rejected key: skipped — no HEADSHELL_TORZNAB_URL");
        return;
    };
    if std::env::var("HEADSHELL_TORZNAB_KEY").is_err() {
        eprintln!("rejected key: skipped — the install asks for no key");
        return;
    }
    let Ok(client) = Torznab::new(&url, "definitely-a-wrong-key") else {
        panic!("HEADSHELL_TORZNAB_URL is invalid");
    };

    match client.search("radiohead", 5).await {
        Ok(outcome) => panic!(
            "a wrong key was accepted and {} results came back — a refusal may have been \
             silently turned into an empty result",
            outcome.releases.len()
        ),
        Err(error) => {
            let message = error.to_string();
            if is_unreachable(&message) {
                eprintln!("rejected key: skipped — could not reach the indexer: {message}");
                return;
            }
            assert!(
                message.contains("refused") || message.contains("HTTP"),
                "the refusal must be said clearly: {message}"
            );
        }
    }
}
