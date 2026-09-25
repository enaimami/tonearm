//! Importing from data export files.
//!
//! **Invariant #1:** importing is done from the export files the user
//! downloaded under their GDPR data portability right, not from a provider's
//! API.

pub mod archive;
pub mod spotify;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::model::{ExportKind, Listen};

pub use archive::{DirArchive, ExportArchive, MemoryArchive, ZipArchive};

/// Why a record did not become a `Listen`.
///
/// No silent dropping — every skipped record is counted under a reason and
/// reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Podcast/audiobook — not music.
    NotMusic,
    /// No track title.
    MissingTitle,
    /// No artist name.
    MissingArtist,
    /// The timestamp could not be read.
    BadTimestamp,
    /// The `ms_played` field is missing or meaningless.
    BadDuration,
}

impl SkipReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotMusic => "not_music",
            Self::MissingTitle => "missing_title",
            Self::MissingArtist => "missing_artist",
            Self::BadTimestamp => "bad_timestamp",
            Self::BadDuration => "bad_duration",
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The summary of an import. Every operation that can partly succeed returns
/// a summary like this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSummary {
    /// The name of the source (zip path or directory).
    pub source: String,
    /// Which export format it was parsed as.
    pub export: ExportKind,
    /// How many files of this format were found in the archive.
    pub files_matched: usize,
    /// How many raw records these files held in total.
    pub records_total: usize,
    /// How many became a `Listen`.
    pub listens: usize,
    /// The skipped ones, by reason.
    pub skipped: BTreeMap<SkipReason, usize>,
    /// How many had an ISRC from the export itself (the first link of the
    /// identity chain).
    pub with_isrc: usize,
    /// How many had a provider track id.
    pub with_provider_id: usize,
}

impl ImportSummary {
    /// The total number of skipped records.
    #[must_use]
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }

    /// Copies the counters into the diagnostics recorder.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        recorder.set("import.files_matched", as_i64(self.files_matched));
        recorder.set("import.records_total", as_i64(self.records_total));
        recorder.set("import.listens", as_i64(self.listens));
        recorder.set("import.with_isrc", as_i64(self.with_isrc));
        recorder.set("import.with_provider_id", as_i64(self.with_provider_id));
        for (reason, count) in &self.skipped {
            recorder.set(format!("import.skipped.{reason}"), as_i64(*count));
        }
    }
}

fn as_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The output of an import: listens + a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    pub listens: Vec<Listen>,
    pub summary: ImportSummary,
}

/// The common interface of the parsers. One for each export format.
pub(crate) trait ExportParser {
    /// The format this parser handles.
    fn kind(&self) -> ExportKind;

    /// Which entries in the archive belong to this format.
    fn matching_entries(&self, entries: &[String]) -> Vec<String>;

    /// Parses a single entry and writes it into the sink.
    fn parse_entry(&self, entry: &str, body: &[u8], sink: &mut ParseSink) -> Result<()>;
}

/// Collects the listens and skip reasons during parsing.
#[derive(Debug, Default)]
pub(crate) struct ParseSink {
    pub(crate) listens: Vec<Listen>,
    pub(crate) records_total: usize,
    pub(crate) skipped: BTreeMap<SkipReason, usize>,
}

impl ParseSink {
    pub(crate) fn accept(&mut self, listen: Listen) {
        self.records_total += 1;
        self.listens.push(listen);
    }

    pub(crate) fn skip(&mut self, reason: SkipReason) {
        self.records_total += 1;
        *self.skipped.entry(reason).or_insert(0) += 1;
    }
}

/// Determines which export format an archive is, from the file names inside
/// it.
///
/// # Errors
/// [`ErrorKind::UnsupportedExport`] if no known format is found — what was
/// found in the archive is written into the error text, so the user sees they
/// gave the wrong zip.
pub fn detect(archive: &dyn ExportArchive) -> Result<ExportKind> {
    let entries = archive.entry_names();
    for parser in parsers() {
        if !parser.matching_entries(&entries).is_empty() {
            return Ok(parser.kind());
        }
    }
    let sample: Vec<&str> = entries.iter().take(10).map(String::as_str).collect();
    Err(Error::new(
        Stage::ImportDetect,
        ErrorKind::UnsupportedExport {
            detail: format!(
                "there is no recognised history file in {} ({} entries). First entries: {}",
                archive.source_label(),
                entries.len(),
                if sample.is_empty() {
                    "(the archive is empty)".to_owned()
                } else {
                    sample.join(", ")
                }
            ),
        },
    ))
}

fn parsers() -> Vec<Box<dyn ExportParser>> {
    vec![
        Box::new(spotify::ExtendedParser),
        Box::new(spotify::AccountParser),
    ]
}

/// Imports an archive.
///
/// The format is detected automatically. Canonical identity resolution is not
/// done here — that is a separate stage ([`crate::identity`]), so what each
/// step produced can be reported separately.
///
/// # Errors
/// If the format is not recognised, the archive cannot be read or a file
/// cannot be parsed at all.
pub fn import(archive: &mut dyn ExportArchive) -> Result<ImportOutcome> {
    let export = detect(&*archive)?;
    let entries = archive.entry_names();
    let parser = parsers()
        .into_iter()
        .find(|p| p.kind() == export)
        .ok_or_else(|| {
            Error::new(
                Stage::ImportDetect,
                ErrorKind::UnsupportedExport {
                    detail: format!("no parser for {export}"),
                },
            )
        })?;

    let matched = parser.matching_entries(&entries);
    let mut sink = ParseSink::default();
    for entry in &matched {
        let body = archive.read_entry(entry)?;
        tracing::debug!(entry, bytes = body.len(), "parsing an export entry");
        parser.parse_entry(entry, &body, &mut sink)?;
    }

    let with_isrc = sink
        .listens
        .iter()
        .filter(|l| l.track.isrc.is_some())
        .count();
    let with_provider_id = sink
        .listens
        .iter()
        .filter(|l| l.track.provider_track_id.is_some())
        .count();

    let summary = ImportSummary {
        source: archive.source_label(),
        export,
        files_matched: matched.len(),
        records_total: sink.records_total,
        listens: sink.listens.len(),
        skipped: sink.skipped,
        with_isrc,
        with_provider_id,
    };
    tracing::info!(
        records = summary.records_total,
        listens = summary.listens,
        skipped = summary.skipped_total(),
        "import finished"
    );
    Ok(ImportOutcome {
        listens: sink.listens,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_what_it_saw_when_unsupported() {
        let archive = MemoryArchive::new("empty.zip").with_entry("readme.txt", b"hello".to_vec());
        let err = detect(&archive).unwrap_err();
        assert_eq!(err.stage(), Stage::ImportDetect);
        let text = err.chain_text();
        assert!(text.contains("readme.txt"), "{text}");
    }
}
