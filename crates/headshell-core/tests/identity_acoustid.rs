//! The chain's 4th link, **against the real AcoustID** (Phase 2 §2.3 /
//! D-046).
//!
//! What is tested with the fake client is the logic: how the body is built,
//! how the response is decoded, which error is written to which stage. What
//! is tested here is something else — that the fingerprint string we produce
//! **is really accepted by AcoustID**. No fake can show this: if the encoding
//! or the compression were off by a bit, the fake client would still say "no
//! match", while the real service would say "invalid fingerprint".
//!
//! **Three failures are kept apart** (K9 / D-043):
//! - **No key** → skipped, the reason is written. A missing configuration, not
//!   a flaw.
//! - **Cannot reach it** → skipped, the reason is written.
//! - **Reaching it and getting the unexpected** → fails.
//!
//! To run it on an install that has a key:
//! `headshell secret set identity:acoustid api_key <key>` — or the
//! `HEADSHELL_ACOUSTID_KEY` environment variable directly.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(all(feature = "fingerprint", feature = "http-client"))]

use std::path::PathBuf;
use std::sync::Arc;

use headshell_core::identity::FingerprintLookup;
use headshell_core::identity::acoustid::AcoustIdLookup;
use headshell_core::identity::fingerprint::fingerprint_file;

/// A synthetic fixture — how it is made is written in
/// `fixtures/audio/README.md`.
///
/// It has **no counterpart, and must not have one**, in the AcoustID
/// database: what is tested here is not recognising a known track but that
/// the difference between "no match found" and "the question could not be
/// asked" is reported correctly.
fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audio/fingerprint_sample.flac")
}

/// Can api.acoustid.org be reached over TCP.
fn acoustid_reachable() -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = ("api.acoustid.org", 443).to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).is_ok()
    })
}

/// Says whether the test can run; if it cannot, **writes the reason**.
fn lookup_or_skip(test: &str) -> Option<AcoustIdLookup> {
    let key = std::env::var("HEADSHELL_ACOUSTID_KEY").unwrap_or_default();
    let http = headshell_core::net::default_http_client().ok()?;
    let lookup = AcoustIdLookup::new(http)
        .with_api_key(key)
        .with_user_agent("headshell-tests/0.0.1 ( https://github.com/headshell/headshell )");

    // Whether the key exists is read from the `Debug` output: the value is
    // written nowhere (D-042).
    if !format!("{lookup:?}").contains("api_key_set: true") {
        eprintln!(
            "{test}: no AcoustID key — skipped (this is not a failure; \
             `HEADSHELL_ACOUSTID_KEY` or `headshell secret set identity:acoustid api_key`)"
        );
        return None;
    }
    if !acoustid_reachable() {
        eprintln!("{test}: could not reach api.acoustid.org:443 — skipped (taken as no network)");
        return None;
    }
    Some(lookup)
}

/// Can AcoustID parse the fingerprint we produce.
///
/// This is the real claim: if the service does not say "invalid
/// fingerprint", the encoding, the compression and the base64 alphabet are
/// right. Whether a match is found is a separate question — a synthetic sound
/// not being in the database is the **expected** result.
#[tokio::test]
async fn acoustid_accepts_the_fingerprint_we_produce() {
    let Some(lookup) = lookup_or_skip("acoustid_accepts_the_fingerprint") else {
        return;
    };
    let print = fingerprint_file(&sample()).expect("the fixture must yield a fingerprint");

    let found = lookup
        .recordings_by_fingerprint(&print)
        .await
        .expect("AcoustID must not reject the fingerprint");

    eprintln!(
        "AcoustID accepted the {}-second fingerprint and returned {} candidates",
        print.duration_secs,
        found.len()
    );
    // A synthetic sound: no candidates coming back is the right result. If some
    // do, we do not fail either — the database grows and one day a match may
    // turn up; the only wrong thing would be the service **not being able to
    // parse** the fingerprint.
    for candidate in &found {
        assert!(
            (0.0..=1.0).contains(&candidate.score),
            "score outside 0–1: {}",
            candidate.score
        );
    }
}

/// Does the fixture the unit tests rest on still tell the truth.
///
/// `fixtures/identity/acoustid_lookup.json` is the input of every AcoustID
/// unit test. It is a frozen file, and every frozen fixture can turn into a
/// lie over time: the service changes its schema, the fixture does not, the
/// unit tests stay green and **production falls over**. That is exactly what
/// happened in D-046 — the `"duration"` field had been written as an integer
/// by hand, the service was sending a decimal, and the first real match would
/// have fallen over with a JSON error.
///
/// This test closes that class: it fetches the live response and compares it
/// with the fixture **type by type, field by field**. The values may change
/// (the catalog is a living thing); what must not change is the shape.
#[tokio::test]
async fn the_committed_fixture_still_matches_what_the_service_sends() {
    const FIXTURE: &str = include_str!("../../../fixtures/identity/acoustid_lookup.json");
    /// The AcoustID record the fixture was taken from — `Şebnem Ferah — Sil
    /// Baştan`.
    const TRACK_ID: &str = "5e45e8ba-c6d9-4782-a20e-5e897f528d23";

    let Some(_lookup) = lookup_or_skip("the_committed_fixture_still_matches") else {
        return;
    };
    let key = std::env::var("HEADSHELL_ACOUSTID_KEY").unwrap_or_default();

    let http = headshell_core::net::default_http_client().expect("http-client is on");
    let url = format!(
        "https://api.acoustid.org/v2/lookup?client={key}&trackid={TRACK_ID}\
         &meta=recordings&format=json"
    );
    let response = match http
        .send(&headshell_core::net::HttpRequest::get(&url))
        .await
    {
        Ok(response) => response,
        Err(err) => {
            eprintln!(
                "the_committed_fixture_still_matches: could not reach it — skipped\n{}",
                err.chain_text()
            );
            return;
        }
    };

    let live: serde_json::Value =
        serde_json::from_slice(&response.body).expect("the live response must be JSON");
    let frozen: serde_json::Value =
        serde_json::from_str(FIXTURE).expect("the fixture must be JSON");

    let mut drift = Vec::new();
    compare_shape("", &frozen, &live, &mut drift);
    assert!(
        drift.is_empty(),
        "the AcoustID schema has drifted from the fixture — the unit tests may no longer \
         be measuring the truth:\n  {}\nlive response:\n{live:#}",
        drift.join("\n  ")
    );
    eprintln!("the fixture schema matches the live response ({TRACK_ID})");
}

/// Compares two JSON trees **at the type level**; writes the differences to
/// `drift`.
///
/// It compares types, not values: a recording's duration may be corrected,
/// its title may change — none of that concerns us. `309.0` becoming `309`,
/// or a field disappearing, does.
///
/// Only the fields the fixture knows are looked for: the service **adding** a
/// new field does not break us (`serde` ignores what it does not know),
/// removing one does.
fn compare_shape(
    path: &str,
    frozen: &serde_json::Value,
    live: &serde_json::Value,
    drift: &mut Vec<String>,
) {
    use serde_json::Value;
    let at = if path.is_empty() { "<root>" } else { path };
    match (frozen, live) {
        (Value::Object(frozen_map), Value::Object(live_map)) => {
            for (key, frozen_value) in frozen_map {
                match live_map.get(key) {
                    Some(live_value) => {
                        compare_shape(&format!("{path}.{key}"), frozen_value, live_value, drift);
                    }
                    None => drift.push(format!(
                        "{at}.{key}: in the fixture, missing from the live response"
                    )),
                }
            }
        }
        (Value::Array(frozen_items), Value::Array(live_items)) => {
            // The first item is representative: what matters is not the array's length
            // but the shape of its items.
            match (frozen_items.first(), live_items.first()) {
                (Some(frozen_first), Some(live_first)) => {
                    compare_shape(&format!("{path}[0]"), frozen_first, live_first, drift);
                }
                (Some(_), None) => drift.push(format!(
                    "{at}: the fixture is filled, the live response is empty"
                )),
                _ => {}
            }
        }
        // For numbers the integer/decimal distinction **matters**: `serde` cannot
        // decode `309.0` into an integer field, and that was exactly the flaw behind
        // D-046.
        (Value::Number(frozen_num), Value::Number(live_num)) => {
            if frozen_num.is_f64() != live_num.is_f64() {
                drift.push(format!(
                    "{at}: the number kind changed (fixture {}, live {})",
                    if frozen_num.is_f64() {
                        "decimal"
                    } else {
                        "integer"
                    },
                    if live_num.is_f64() {
                        "decimal"
                    } else {
                        "integer"
                    },
                ));
            }
        }
        (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Null, Value::Null) => {}
        (frozen_other, live_other) => drift.push(format!(
            "{at}: the type changed (fixture {}, live {})",
            kind_of(frozen_other),
            kind_of(live_other)
        )),
    }
}

fn kind_of(value: &serde_json::Value) -> &'static str {
    use serde_json::Value;
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The **control** for the test above: AcoustID does not accept just any
/// string.
///
/// `acoustid_accepts_the_fingerprint_we_produce` says "the service did not
/// reject it". If the service rejected nothing, that claim would be empty and
/// its emptiness would be invisible — the test would stay green even if our
/// compressor broke. This test closes that gap: it sends a deliberately
/// corrupted string and expects it **to be rejected**.
///
/// The request is built by hand because a corrupt string cannot be passed
/// through our own client: `Fingerprint::to_acoustid_string` always produces
/// structurally valid output. What is tested is not our code anyway but
/// whether the service makes the distinction (measured on 2026-09-02: `code
/// 3, invalid fingerprint`).
#[tokio::test]
async fn acoustid_rejects_a_corrupted_fingerprint_so_the_positive_test_means_something() {
    let Some(lookup) = lookup_or_skip("acoustid_rejects_a_corrupted_fingerprint") else {
        return;
    };
    // We cannot read the key from `lookup` (hidden on purpose, D-042); we take
    // it from the environment — `lookup_or_skip` already verified it is set.
    let key = std::env::var("HEADSHELL_ACOUSTID_KEY").unwrap_or_default();
    drop(lookup);

    let print = fingerprint_file(&sample()).expect("the fixture must yield a fingerprint");
    let valid = print.to_acoustid_string();
    // A piece is cut out of the middle: its alphabet is still valid, its
    // structure is not.
    let corrupted = format!("{}{}", &valid[..40], &valid[80..]);
    assert_ne!(
        corrupted, valid,
        "the corruption must really change something"
    );

    let http = headshell_core::net::default_http_client().expect("http-client is on");
    // The body is built by hand. No escaping needed: the key and the fingerprint
    // only carry characters from the URL-safe alphabet.
    let body = format!(
        "client={key}&duration={}&fingerprint={corrupted}&meta=recordings",
        print.duration_secs
    );
    let request = headshell_core::net::HttpRequest::post_form(
        "https://api.acoustid.org/v2/lookup",
        body.into_bytes(),
    );
    let response = match http.send(&request).await {
        Ok(response) => response,
        Err(err) => {
            eprintln!(
                "acoustid_rejects_a_corrupted_fingerprint: could not reach it — skipped\n{}",
                err.chain_text()
            );
            return;
        }
    };

    let text = response.text_lossy();
    eprintln!("response to the corrupted fingerprint: {text}");
    assert!(
        text.contains("invalid fingerprint"),
        "the service accepted a corrupted fingerprint; the `accepted` test no longer measures anything: {text}"
    );
}

/// An invalid key must not be reported as "no match".
///
/// This test **needs no key**: it goes with a deliberately broken key and
/// expects the service to refuse. K9's most repeated lesson is measured here
/// — the real service returns an error inside a `200` body, and a client
/// that swallowed it would send the user off to look for the fault in their
/// file.
#[tokio::test]
async fn a_rejected_key_is_an_error_not_an_empty_result() {
    if !acoustid_reachable() {
        eprintln!(
            "a_rejected_key_is_an_error: could not reach api.acoustid.org:443 — \
             skipped (taken as no network)"
        );
        return;
    }
    let Ok(http) = headshell_core::net::default_http_client() else {
        return;
    };
    let lookup = AcoustIdLookup::new(Arc::clone(&http))
        .with_api_key("invalid-key-test")
        .with_user_agent("headshell-tests/0.0.1 ( https://github.com/headshell/headshell )");
    let print = fingerprint_file(&sample()).expect("the fixture must yield a fingerprint");

    let err = lookup
        .recordings_by_fingerprint(&print)
        .await
        .expect_err("an invalid key must give an error, not an empty list");
    let text = err.chain_text();
    eprintln!("{text}");
    assert!(text.starts_with("STEP: IDENTITY_RESOLVE"), "{text}");
    assert!(text.to_lowercase().contains("api key"), "{text}");
}
