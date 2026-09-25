//! The core data model: the listen event and the track reference.

use serde::{Deserialize, Serialize};

use crate::ids::{CanonicalId, Isrc, ProviderId, ProviderTrackId};

/// A track in its non-canonical form — as read from the export file.
///
/// This is the input of identity resolution; its output is a
/// [`CanonicalId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRef {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    /// The track's full duration (if known). The distinguishing field for fuzzy
    /// matching.
    pub duration_ms: Option<u64>,
    /// The first link of the identity chain. Worth its weight in gold if the
    /// export has it.
    pub isrc: Option<Isrc>,
    /// The export's own id (`spotify:track:...`). Not canonical.
    pub provider_track_id: Option<ProviderTrackId>,
}

impl TrackRef {
    /// The plainest form, with only artist+title known.
    #[must_use]
    pub fn new(artist: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            artist: artist.into(),
            title: title.into(),
            album: None,
            duration_ms: None,
            isrc: None,
            provider_track_id: None,
        }
    }

    #[must_use]
    pub fn with_album(mut self, album: Option<String>) -> Self {
        self.album = album;
        self
    }

    #[must_use]
    pub fn with_duration_ms(mut self, duration_ms: Option<u64>) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    #[must_use]
    pub fn with_isrc(mut self, isrc: Option<Isrc>) -> Self {
        self.isrc = isrc;
        self
    }

    #[must_use]
    pub fn with_provider_track_id(mut self, id: Option<ProviderTrackId>) -> Self {
        self.provider_track_id = id;
        self
    }

    /// One human-readable line: `Artist - Title`.
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{} - {}", self.artist, self.title)
    }

    /// Parses a one-line query of the form `"Radiohead - Creep"`.
    ///
    /// The separator is the first ` - ` sequence; the spaced form is looked for
    /// because an artist's name may contain a hyphen. The CLI does not do this
    /// itself — parsing is a data transformation and belongs in the core.
    ///
    /// # Errors
    /// If there is no separator or either side is empty.
    pub fn parse_query(input: &str) -> crate::Result<Self> {
        let invalid = |detail: String| {
            crate::Error::new(
                crate::diag::Stage::IdentityResolve,
                crate::ErrorKind::InvalidInput { detail },
            )
        };
        let (artist, title) = input.split_once(" - ").ok_or_else(|| {
            invalid(format!(
                "{input:?} is not of the form 'Artist - Title' (separator: space-hyphen-space)"
            ))
        })?;
        let (artist, title) = (artist.trim(), title.trim());
        if artist.is_empty() || title.is_empty() {
            return Err(invalid(format!(
                "the artist or the title is empty in {input:?}"
            )));
        }
        Ok(Self::new(artist, title))
    }
}

/// A single listen event. The glossary's `listen`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listen {
    /// What was listened to (in its raw form).
    pub track: TrackRef,
    /// When it started/ended — depends on the export, normalised to UTC.
    pub played_at: jiff::Timestamp,
    /// How many milliseconds were played. Skip detection and the "real listen"
    /// threshold rest on this.
    pub ms_played: u64,
    /// Where the record came from.
    pub source: ListenSource,
    /// The canonical identity, if resolved. In Phase 0 it is filled in after
    /// import.
    pub canonical_id: Option<CanonicalId>,
}

impl Listen {
    /// Is this listen a counted play?
    ///
    /// [`PlayRule`] decides — a single definition in the core (D-008).
    #[must_use]
    pub fn counts_as_play(&self, rule: PlayRule) -> bool {
        rule.counts(self.ms_played, self.track.duration_ms)
    }
}

/// The threshold Spotify counts as "listened"; it is the default because it
/// is an industry habit, but it is not hidden — it can be changed with
/// [`PlayRule`].
pub const DEFAULT_MIN_MS_PLAYED: u64 = 30_000;

/// The rule that decides whether a listen event is a **counted play**.
///
/// D-008: this concept is defined in one place in the core. When `stats`
/// applied the threshold and `library search` counted raw events, the same
/// fixture came out as "18 plays" and "9 plays". Now both surfaces call this
/// rule.
///
/// The names were separated too, so the two numbers are not mixed up:
/// - **`play_count`** — the number of plays that pass this rule, shown to the
///   user.
/// - **`listen_events`** — the raw event count; only in `diag` and when
///   debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayRule {
    /// Plays shorter than this count as skips.
    pub min_ms_played: u64,
}

impl Default for PlayRule {
    fn default() -> Self {
        Self {
            min_ms_played: DEFAULT_MIN_MS_PLAYED,
        }
    }
}

impl PlayRule {
    #[must_use]
    pub const fn new(min_ms_played: u64) -> Self {
        Self { min_ms_played }
    }

    /// The scrobble convention: past the threshold **or** at least half of the
    /// track.
    ///
    /// The half-track arm only works if the duration is known; counting an
    /// unknown duration as "listened to half" inflates the number. Phase 0's
    /// Spotify export carries no durations, so in practice the threshold arm
    /// decides — the rule comes into play once local files arrive (Phase 1).
    #[must_use]
    pub const fn counts(self, ms_played: u64, duration_ms: Option<u64>) -> bool {
        if ms_played >= self.min_ms_played {
            return true;
        }
        match duration_ms {
            Some(duration) if duration > 0 => ms_played.saturating_mul(2) >= duration,
            _ => false,
        }
    }
}

/// Where a listen comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListenSource {
    /// Imported from a data export file.
    Import { export: ExportKind },
    /// Played with `headshell` through a provider.
    Playback { provider: ProviderId },
}

/// Which provider's export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportKind {
    /// Spotify "Extended streaming history" (`Streaming_History_Audio_*.json`).
    SpotifyExtended,
    /// Spotify account data (`StreamingHistory*.json`) — only the last year.
    SpotifyAccount,
    /// Apple Music privacy export.
    AppleMusic,
    /// Google Takeout / YouTube Music.
    GoogleTakeout,
}

impl ExportKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SpotifyExtended => "spotify_extended",
            Self::SpotifyAccount => "spotify_account",
            Self::AppleMusic => "apple_music",
            Self::GoogleTakeout => "google_takeout",
        }
    }
}

impl std::fmt::Display for ExportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listen(ms_played: u64, duration_ms: Option<u64>) -> Listen {
        Listen {
            track: TrackRef::new("Radiohead", "Creep").with_duration_ms(duration_ms),
            played_at: jiff::Timestamp::UNIX_EPOCH,
            ms_played,
            source: ListenSource::Import {
                export: ExportKind::SpotifyExtended,
            },
            canonical_id: None,
        }
    }

    #[test]
    fn play_threshold_is_explicit() {
        let l = listen(25_000, None);
        assert!(!l.counts_as_play(PlayRule::new(30_000)));
        assert!(l.counts_as_play(PlayRule::new(20_000)));
    }

    #[test]
    fn half_a_short_track_counts_even_below_the_threshold() {
        let rule = PlayRule::default();
        // 25 s of a 40 s track: below the threshold but more than half.
        assert!(rule.counts(25_000, Some(40_000)));
        // The same duration, a long track: it does not reach half, not counted.
        assert!(!rule.counts(25_000, Some(240_000)));
        // If the duration is unknown the half-track arm never applies.
        assert!(!rule.counts(25_000, None));
        // A zero duration must not produce a division or an inflated count.
        assert!(!rule.counts(1, Some(0)));
    }

    #[test]
    fn threshold_alone_is_enough_regardless_of_duration() {
        let rule = PlayRule::default();
        assert!(rule.counts(30_000, None));
        assert!(rule.counts(30_000, Some(600_000)));
    }

    #[test]
    fn parse_query_splits_on_the_first_spaced_dash() {
        let track = TrackRef::parse_query("Radiohead - Creep").unwrap();
        assert_eq!(track.artist, "Radiohead");
        assert_eq!(track.title, "Creep");

        let dashed = TrackRef::parse_query("Sault - 9 - Reprise").unwrap();
        assert_eq!(dashed.artist, "Sault");
        assert_eq!(dashed.title, "9 - Reprise");
    }

    #[test]
    fn parse_query_rejects_input_without_a_separator() {
        let err = TrackRef::parse_query("only a title").unwrap_err();
        assert!(
            err.chain_text().contains("Artist - Title"),
            "{}",
            err.chain_text()
        );
        assert!(TrackRef::parse_query(" - Creep").is_err());
        assert!(TrackRef::parse_query("Radiohead - ").is_err());
    }

    #[test]
    fn listen_source_round_trips_as_tagged_json() {
        let source = ListenSource::Import {
            export: ExportKind::SpotifyExtended,
        };
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, r#"{"kind":"import","export":"spotify_extended"}"#);
        let back: ListenSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }
}
