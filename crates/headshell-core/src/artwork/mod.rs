//! Cover art (D-076): finding a track's cover, keeping it, handing it to the
//! interface.
//!
//! ## The chain
//!
//! 1. **Where the track lives.** A local file: the picture in its tags (ID3
//!    `APIC`, FLAC `PICTURE`, MP4 `covr`, Vorbis `METADATA_BLOCK_PICTURE`),
//!    then an image in its folder (`cover`, `folder`, `front`). A remote
//!    provider or a plugin: its own cover ([`Provider::artwork`]). This link
//!    needs no `--online`: it is the server the audio already comes from.
//! 2. **Third parties — only online** (`--online`, `HEADSHELL_ONLINE=1`): the
//!    recording's MBID (the identity chain, run online: queued tracks carry
//!    none) → its releases at MusicBrainz, the one whose title is the track's
//!    album preferred, an official one next → the Cover Art Archive: the
//!    release's front, then its release group's.
//!
//! A link that fails — a broken picture, a plugin that throws, a server that
//! errors — does not end the chain: the next link is asked, and what failed
//! is written into the item's notes (K9). Nothing found is written back into
//! the listening history: an MBID found here stays here.
//!
//! ## What is kept
//!
//! Per album ([`ArtworkKey`]), in `<data_dir>/artwork/`: two variants —
//! `label` (320 px) and `thumb` (96 px) — named by their content hash, and
//! `index.json`. "Not found" is kept too, for [`cache::NOT_FOUND_TTL`], then
//! asked again: the archive grows. "Not looked up because offline" and errors
//! are not kept: they are asked again next time.
//!
//! ## Who calls it
//!
//! The command (`headshell artwork`, [`crate::session::Session::artwork`])
//! walks the chain for the tracks a query finds, in play order, and waits.
//! The playing session ([`crate::playback::LiveSession`]) hands its queue to
//! a background [`ArtworkWorker`] instead: the HTTP client and the MusicBrainz
//! limiter block the thread they run on, and on the core's thread they would
//! hold up pause and next.

pub(crate) mod cache;
mod chain;
mod embedded;
pub(crate) mod image;
pub(crate) mod lookup;
mod worker;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::{DiagReport, Stage};
use crate::error::{Result, io_err};
use crate::identity::normalize;
pub use crate::ids::ArtworkKey;
use crate::ids::ProviderId;
use crate::model::TrackRef;
#[cfg(doc)]
use crate::provider::Provider;

pub(crate) use chain::{Chain, Links, Online, Outcome};
pub(crate) use embedded::file_cover;
pub use worker::ArtworkWorker;

/// The edge, in pixels, providers are asked for. The label variant is 320 px;
/// a little more leaves room for the resize.
pub const REQUEST_EDGE: u32 = 500;

/// Where a cover came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkSource {
    /// The picture in the audio file's tags.
    Embedded,
    /// An image next to the audio file (`cover.jpg`).
    Folder,
    /// The provider or plugin the track plays from.
    Provider,
    /// The Cover Art Archive, through MusicBrainz.
    CoverArtArchive,
}

impl ArtworkSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Folder => "folder",
            Self::Provider => "provider",
            Self::CoverArtArchive => "cover_art_archive",
        }
    }
}

/// What happened to one track's cover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ArtworkStatus {
    /// Found; the images are in the cache under the item's key.
    Found { source: ArtworkSource },
    /// Every link allowed was asked; none had a cover. `detail` says where
    /// the chain ended.
    NotFound { detail: String },
    /// The local links had nothing, and the third parties were **not asked**:
    /// offline. A different diagnosis from "not found" (K9).
    NotCheckedOffline,
    /// A link failed and none after it found a cover: the error chain.
    Failed { chain: String },
    /// Not reached yet — the background worker is still on its way.
    Pending,
}

/// One track's line in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkItem {
    /// The position in play order.
    pub index: usize,
    /// The key the cover is kept and asked for under.
    pub key: ArtworkKey,
    pub provider: ProviderId,
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    #[serde(flatten)]
    pub status: ArtworkStatus,
    /// What went wrong on the way when the chain went on anyway: a plugin
    /// that threw before the archive answered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// The counts (K9): how many tracks, and what happened to them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkSummary {
    pub tracks: usize,
    pub embedded: usize,
    pub folder: usize,
    pub provider: usize,
    pub cover_art_archive: usize,
    pub not_found: usize,
    pub not_checked_offline: usize,
    pub failed: usize,
    pub pending: usize,
}

impl ArtworkSummary {
    /// Counts the items.
    #[must_use]
    pub fn of(items: &[ArtworkItem]) -> Self {
        let mut summary = Self {
            tracks: items.len(),
            ..Self::default()
        };
        for item in items {
            match &item.status {
                ArtworkStatus::Found { source } => match source {
                    ArtworkSource::Embedded => summary.embedded += 1,
                    ArtworkSource::Folder => summary.folder += 1,
                    ArtworkSource::Provider => summary.provider += 1,
                    ArtworkSource::CoverArtArchive => summary.cover_art_archive += 1,
                },
                ArtworkStatus::NotFound { .. } => summary.not_found += 1,
                ArtworkStatus::NotCheckedOffline => summary.not_checked_offline += 1,
                ArtworkStatus::Failed { .. } => summary.failed += 1,
                ArtworkStatus::Pending => summary.pending += 1,
            }
        }
        summary
    }

    /// How many have a cover.
    #[must_use]
    pub const fn found(&self) -> usize {
        self.embedded + self.folder + self.provider + self.cover_art_archive
    }
}

/// The covers of a list of tracks: what `headshell artwork` prints, and what
/// the interface asks for its queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtworkReport {
    /// What was looked at: the query, the file, or `queue`.
    pub subject: String,
    /// Were third parties allowed (`--online`)?
    pub online: bool,
    pub items: Vec<ArtworkItem>,
    pub summary: ArtworkSummary,
    /// The images written with `--out`, one per key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub written: Vec<PathBuf>,
    /// The run's diagnostics — only for the command; the queue's report is
    /// not a run of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diag: Option<DiagReport>,
}

/// Which of a cover's two sizes: the record's label, or a queue row's
/// thumbnail. They are asked for apart — a queue of thirty albums needs
/// thirty thumbnails but one label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkVariant {
    Label,
    Thumb,
}

/// One size of a cover, ready for the webview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkPicture {
    pub key: ArtworkKey,
    pub variant: ArtworkVariant,
    /// A `data:` URI. The CSP loads no remote image: this is the only way a
    /// cover gets on screen.
    pub uri: String,
}

/// The key a track's cover is kept under: its album's, or its own when it has
/// no album. The artist and the album are normalised the way the identity
/// chain normalises them, so `Pablo Honey` and `pablo honey` are one cover.
#[must_use]
pub fn key_for(track: &TrackRef) -> ArtworkKey {
    let artist = normalize::normalize_artist(&track.artist);
    match track
        .album
        .as_deref()
        .map(normalize::normalize_text)
        .filter(|album| !album.is_empty())
    {
        Some(album) => ArtworkKey::album(&artist, &album),
        None => ArtworkKey::track(&artist, &normalize::normalize_text(&track.title)),
    }
}

/// Writes one image per found album into `dir` (`headshell artwork
/// --out`): the label variant, named after the album's first track in the
/// list — `01 Artist - Album.png`.
///
/// # Errors
/// If the directory cannot be created, or an image cannot be read or written.
pub(crate) fn write_covers(
    store: &cache::ArtworkCache,
    items: &[ArtworkItem],
    dir: &Path,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir).map_err(|source| io_err(Stage::ArtworkStore, dir, source))?;
    let mut written = Vec::new();
    let mut done = HashSet::new();
    for item in items {
        if !matches!(item.status, ArtworkStatus::Found { .. }) || !done.insert(item.key.clone()) {
            continue;
        }
        let Some(cache::Entry::Found { label, .. }) = store.get(&item.key) else {
            continue;
        };
        let (bytes, format) = store.read_image(label)?;
        let name = format!(
            "{:02} {} - {}.{}",
            item.index + 1,
            item.artist,
            item.album.as_deref().unwrap_or(&item.title),
            format.extension()
        );
        let path = dir.join(file_name_safe(&name));
        std::fs::write(&path, bytes)
            .map_err(|source| io_err(Stage::ArtworkStore, &path, source))?;
        written.push(path);
    }
    Ok(written)
}

/// A name every file system takes: the characters Windows refuses, and
/// control characters, become `_` (D-070).
fn file_name_safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned.trim().trim_start_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_album_is_one_key_whatever_the_case() {
        let a = TrackRef::new("Radiohead", "Creep").with_album(Some("Pablo Honey".to_owned()));
        let b = TrackRef::new("RADIOHEAD", "Anyone Can Play Guitar")
            .with_album(Some("pablo honey".to_owned()));
        assert_eq!(key_for(&a), key_for(&b));
        // No album: the track is its own key.
        let single = TrackRef::new("Radiohead", "Creep");
        assert_ne!(key_for(&single), key_for(&a));
        assert!(key_for(&single).as_str().starts_with("track:"));
        // An empty album is no album.
        let empty = TrackRef::new("Radiohead", "Creep").with_album(Some("  ".to_owned()));
        assert_eq!(key_for(&empty), key_for(&single));
    }

    #[test]
    fn the_summary_counts_every_status_once() {
        let item = |status| ArtworkItem {
            index: 0,
            key: ArtworkKey::track("a", "b"),
            provider: ProviderId::new("local"),
            artist: "A".to_owned(),
            title: "B".to_owned(),
            album: None,
            status,
            notes: Vec::new(),
        };
        let items = vec![
            item(ArtworkStatus::Found {
                source: ArtworkSource::Embedded,
            }),
            item(ArtworkStatus::Found {
                source: ArtworkSource::CoverArtArchive,
            }),
            item(ArtworkStatus::NotFound {
                detail: "x".to_owned(),
            }),
            item(ArtworkStatus::NotCheckedOffline),
            item(ArtworkStatus::Failed {
                chain: "STEP: X".to_owned(),
            }),
            item(ArtworkStatus::Pending),
        ];
        let summary = ArtworkSummary::of(&items);
        assert_eq!(summary.tracks, 6);
        assert_eq!(summary.found(), 2);
        assert_eq!(
            (
                summary.not_found,
                summary.not_checked_offline,
                summary.failed,
                summary.pending
            ),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn a_status_is_flat_in_json() {
        let json = serde_json::to_value(ArtworkStatus::Found {
            source: ArtworkSource::Folder,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"status": "found", "source": "folder"})
        );
        let json = serde_json::to_value(ArtworkStatus::NotCheckedOffline).unwrap();
        assert_eq!(json, serde_json::json!({"status": "not_checked_offline"}));
    }

    #[test]
    fn a_cover_file_name_is_safe_everywhere() {
        assert_eq!(
            file_name_safe("01 AC/DC - Back: In Black?.png"),
            "01 AC_DC - Back_ In Black_.png"
        );
        assert_eq!(file_name_safe("..hidden"), "hidden");
    }
}
