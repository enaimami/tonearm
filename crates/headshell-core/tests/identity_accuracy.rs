//! The accuracy measurement of identity resolution.
//!
//! `fixtures/identity/cases.json` is the labelled accuracy set. The rate this
//! test prints is the project's most important metric: after every change
//! that touches matching, look at the number here.
//!
//! Nothing goes online — the catalog is inside the file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use headshell_core::identity::{Candidate, ResolveMethod, Resolver, StaticLookup};
use headshell_core::ids::{CanonicalId, Isrc, Mbid};
use headshell_core::model::TrackRef;
use serde::Deserialize;

/// The lowest accepted accuracy. As the rate goes up, so does this; it is
/// never lowered "to make the test pass" — a drop is a regression (D-009).
const ACCURACY_FLOOR: f64 = 0.97;

#[derive(Debug, Deserialize)]
struct Dataset {
    catalog: Vec<CatalogEntry>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    mbid: String,
    artist: String,
    title: String,
    duration_ms: Option<u64>,
    isrc: Option<String>,
    /// MusicBrainz's disambiguation note. In the real catalog this is the
    /// **only** marker of live recordings; the title stays plain (D-045).
    #[serde(default)]
    disambiguation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Case {
    /// The case class (`live`, `cover`, `turkish`, ...). The rate is printed per
    /// class too.
    class: String,
    note: String,
    artist: String,
    title: String,
    duration_ms: Option<u64>,
    #[serde(default)]
    isrc: Option<String>,
    /// If `None`: it must not be tied to any authority (it must fall back to the
    /// local key).
    expect_mbid: Option<String>,
    /// If given, this link of the chain is expected to resolve it.
    #[serde(default)]
    expect_method: Option<String>,
}

impl Case {
    fn expectation(&self) -> String {
        match (&self.expect_mbid, &self.expect_method) {
            (Some(mbid), _) => mbid.clone(),
            (None, Some(method)) => format!("authority: {method}"),
            (None, None) => "no match".to_owned(),
        }
    }

    /// Does the resolution agree with this case's label?
    fn is_satisfied_by(&self, resolution: &headshell_core::identity::Resolution) -> bool {
        if let Some(method) = &self.expect_method {
            if resolution.method.as_str() != method {
                return false;
            }
        }
        match &self.expect_mbid {
            Some(mbid) => {
                let want = CanonicalId::from_mbid(
                    &Mbid::parse(mbid).expect("the expected mbid must be valid"),
                );
                resolution.canonical_id == want
            }
            // If a method expectation was given we already checked it; if not, it
            // means "must not be tied to any authority".
            None if self.expect_method.is_some() => true,
            None => resolution.method == ResolveMethod::LocalKey,
        }
    }
}

fn load() -> Dataset {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/identity/cases.json"
    );
    let body = std::fs::read(path).unwrap_or_else(|err| panic!("could not read {path}: {err}"));
    serde_json::from_slice(&body).expect("cases.json must be valid")
}

#[tokio::test]
async fn identity_accuracy_over_labelled_cases() {
    let dataset = load();
    let lookup = StaticLookup::new(
        dataset
            .catalog
            .iter()
            .map(|entry| Candidate {
                mbid: Mbid::parse(&entry.mbid).expect("the mbid in the catalog must be valid"),
                artist: entry.artist.clone(),
                title: entry.title.clone(),
                duration_ms: entry.duration_ms,
                isrc: entry.isrc.as_deref().and_then(Isrc::parse),
                disambiguation: entry.disambiguation.clone(),
            })
            .collect(),
    );
    let resolver = Resolver::new(std::sync::Arc::new(lookup));

    let mut correct = 0usize;
    let mut failures = Vec::new();
    // A breakdown by class: the overall rate can hide a collapse in one class.
    let mut by_class: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for case in &dataset.cases {
        let track = TrackRef::new(&case.artist, &case.title)
            .with_duration_ms(case.duration_ms)
            .with_isrc(case.isrc.as_deref().and_then(Isrc::parse));

        let resolution = resolver
            .resolve(&track)
            .await
            .expect("resolution must not fail");

        let entry = by_class.entry(case.class.as_str()).or_default();
        entry.1 += 1;
        if case.is_satisfied_by(&resolution) {
            correct += 1;
            entry.0 += 1;
        } else {
            failures.push(format!(
                "  [{}/{}] {} - {} → {} ({}, confidence {:.2}), expected: {}",
                case.class,
                case.note,
                case.artist,
                case.title,
                resolution.canonical_id,
                resolution.method,
                resolution.confidence,
                case.expectation(),
            ));
        }
    }

    let total = dataset.cases.len();
    let negatives = dataset
        .cases
        .iter()
        .filter(|case| case.expect_mbid.is_none() && case.expect_method.is_none())
        .count();

    // D-009: 100% on an easy set means nothing was measured. The set itself is a
    // contract too; if it shrinks the metric becomes meaningless.
    assert!(
        total >= 60,
        "the accuracy set must contain at least 60 cases, it has {total}"
    );
    assert!(
        negatives >= 15,
        "at least 15 negative cases are needed, there are {negatives}"
    );

    #[expect(clippy::cast_precision_loss, reason = "rate display")]
    let accuracy = correct as f64 / total as f64;
    println!(
        "\nIDENTITY ACCURACY: {correct}/{total} = {:.1}%  ({negatives} negative cases)",
        accuracy * 100.0
    );
    println!("by class:");
    for (class, (ok, seen)) in &by_class {
        let mark = if ok == seen { " " } else { "!" };
        println!("  {mark} {class:<16} {ok:>2}/{seen:<2}");
    }
    if !failures.is_empty() {
        println!("failing cases:\n{}", failures.join("\n"));
    }

    // The threshold is deliberately a little below the current rate: a
    // regression is caught, a small fluctuation does not break the test. Raise
    // the threshold as the rate goes up.
    assert!(
        accuracy >= ACCURACY_FLOOR,
        "accuracy dropped to {:.1}% (threshold {:.1}%)\n{}",
        accuracy * 100.0,
        ACCURACY_FLOOR * 100.0,
        failures.join("\n")
    );
}
