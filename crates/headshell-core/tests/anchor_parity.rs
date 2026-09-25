//! The accuracy set for anchor estimation (PLAN §3.2).
//!
//! **Why a separate fixture file and not a plain Rust test:** in the webview
//! the position is estimated without asking the core, so a second copy of the
//! formula will live in JS. Two copies drift over time, and the drift starts
//! where nobody notices — the progress bar lies by a few hundred
//! milliseconds, nobody complains, and then in Phase 4 **the same formula
//! drives room sync.**
//!
//! `fixtures/anchor/position_cases.json` is the single source of truth both
//! sides read. This file binds the Rust side; when the GUI package is written
//! the JS side will read the same file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use headshell_core::playback::PlaybackAnchor;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    anchor: PlaybackAnchor,
    now: jiff::Timestamp,
    expected_ms: u64,
}

#[derive(serde::Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

fn cases() -> Cases {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/anchor/position_cases.json"
    ));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("could not read {}: {err}", path.display()));
    serde_json::from_str(&text).expect("the accuracy set must parse")
}

#[test]
fn every_case_in_the_shared_truth_set_matches_the_core_formula() {
    let cases = cases();
    assert!(
        cases.cases.len() >= 12,
        "the accuracy set has shrunk: {} cases",
        cases.cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases.cases {
        let got = case.anchor.position_at(case.now);
        if got != case.expected_ms {
            failures.push(format!(
                "  {}: expected {} ms, got {} ms",
                case.name, case.expected_ms, got
            ));
        }
    }

    // All are reported at once: stopping at the first mismatch would hide
    // how many cases are affected when the formula changes.
    assert!(
        failures.is_empty(),
        "{} / {} cases did not match:\n{}",
        failures.len(),
        cases.cases.len(),
        failures.join("\n")
    );
}

/// An accuracy set that only covers the easy path is no lock.
#[test]
fn the_truth_set_covers_the_cases_that_actually_break_a_reimplementation() {
    let cases = cases();
    let all: String = cases
        .cases
        .iter()
        .map(|case| case.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for needle in [
        "paused",    // must not advance
        "buffering", // Buffering must not advance either — the easily missed case
        "rate 0",    // looks like playing but does not advance
        "1.001",     // Phase 4 drift correction
        "backwards", // if the clock jumps back
        "exceed",    // clipping to the duration
        "unknown",   // no clipping without a duration
    ] {
        assert!(
            all.contains(needle),
            "there is no '{needle}' case in the accuracy set:\n{all}"
        );
    }
}
