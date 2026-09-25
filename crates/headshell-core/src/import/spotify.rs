//! Spotify export parsers.
//!
//! There are two formats:
//! - **Extended streaming history** — `Streaming_History_Audio_*.json`, the
//!   whole history, track URIs included. This is the one to ask for.
//! - **Account data** — `StreamingHistory*.json`, only the last 12 months, no
//!   URIs.
//!
//! Note: Spotify exports do not include ISRCs. So the identity chain starts
//! here with `spotify:track:...` and is turned into an MBID in
//! [`crate::identity`].

use serde::Deserialize;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::{ProviderId, ProviderTrackId};
use crate::import::{ExportParser, ParseSink, SkipReason};
use crate::model::{ExportKind, Listen, ListenSource, TrackRef};

/// Spotify's provider name. The core does not *depend* on Spotify; this is
/// only a label saying which name the id coming from the export belongs to.
fn spotify_provider() -> ProviderId {
    ProviderId::new("spotify")
}

/// Parses entries like `Streaming_History_Audio_2023_5.json`.
pub(crate) struct ExtendedParser;

/// An extended history record. Unknown fields are ignored; Spotify extends
/// the schema from time to time, which is why no field is required.
#[derive(Debug, Deserialize)]
struct ExtendedRecord {
    ts: Option<String>,
    ms_played: Option<i64>,
    master_metadata_track_name: Option<String>,
    master_metadata_album_artist_name: Option<String>,
    master_metadata_album_album_name: Option<String>,
    spotify_track_uri: Option<String>,
    /// Set if it is a podcast episode — this record is not music.
    #[serde(default)]
    episode_name: Option<String>,
    /// An audiobook chapter (exports from 2024 onwards).
    #[serde(default)]
    audiobook_title: Option<String>,
}

impl ExportParser for ExtendedParser {
    fn kind(&self) -> ExportKind {
        ExportKind::SpotifyExtended
    }

    fn matching_entries(&self, entries: &[String]) -> Vec<String> {
        let mut matched: Vec<String> = entries
            .iter()
            .filter(|entry| {
                let name = file_name(entry);
                name.starts_with("Streaming_History_Audio") && name.ends_with(".json")
            })
            .cloned()
            .collect();
        matched.sort();
        matched
    }

    fn parse_entry(&self, entry: &str, body: &[u8], sink: &mut ParseSink) -> Result<()> {
        let records: Vec<ExtendedRecord> = parse_json(entry, body)?;
        for record in records {
            if record.episode_name.is_some() || record.audiobook_title.is_some() {
                sink.skip(SkipReason::NotMusic);
                continue;
            }
            let Some(title) = non_empty(record.master_metadata_track_name) else {
                sink.skip(SkipReason::MissingTitle);
                continue;
            };
            let Some(artist) = non_empty(record.master_metadata_album_artist_name) else {
                sink.skip(SkipReason::MissingArtist);
                continue;
            };
            let Some(played_at) = record.ts.as_deref().and_then(parse_rfc3339) else {
                sink.skip(SkipReason::BadTimestamp);
                continue;
            };
            let Some(ms_played) = record.ms_played.and_then(|ms| u64::try_from(ms).ok()) else {
                sink.skip(SkipReason::BadDuration);
                continue;
            };

            let track = TrackRef::new(artist, title)
                .with_album(non_empty(record.master_metadata_album_album_name))
                .with_provider_track_id(
                    record
                        .spotify_track_uri
                        .as_deref()
                        .and_then(track_id_from_uri),
                );
            sink.accept(Listen {
                track,
                played_at,
                ms_played,
                source: ListenSource::Import {
                    export: ExportKind::SpotifyExtended,
                },
                canonical_id: None,
            });
        }
        Ok(())
    }
}

/// The parser for `StreamingHistory_music_0.json` / `StreamingHistory0.json`.
pub(crate) struct AccountParser;

#[derive(Debug, Deserialize)]
struct AccountRecord {
    /// `"2023-05-01 12:34"` — no time zone, local time.
    #[serde(rename = "endTime")]
    end_time: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    #[serde(rename = "trackName")]
    track_name: Option<String>,
    #[serde(rename = "msPlayed")]
    ms_played: Option<i64>,
}

impl ExportParser for AccountParser {
    fn kind(&self) -> ExportKind {
        ExportKind::SpotifyAccount
    }

    fn matching_entries(&self, entries: &[String]) -> Vec<String> {
        let mut matched: Vec<String> = entries
            .iter()
            .filter(|entry| {
                let name = file_name(entry);
                name.starts_with("StreamingHistory") && name.ends_with(".json")
            })
            .cloned()
            .collect();
        matched.sort();
        matched
    }

    fn parse_entry(&self, entry: &str, body: &[u8], sink: &mut ParseSink) -> Result<()> {
        let records: Vec<AccountRecord> = parse_json(entry, body)?;
        for record in records {
            let Some(title) = non_empty(record.track_name) else {
                sink.skip(SkipReason::MissingTitle);
                continue;
            };
            let Some(artist) = non_empty(record.artist_name) else {
                sink.skip(SkipReason::MissingArtist);
                continue;
            };
            let Some(played_at) = record.end_time.as_deref().and_then(parse_naive_minutes) else {
                sink.skip(SkipReason::BadTimestamp);
                continue;
            };
            let Some(ms_played) = record.ms_played.and_then(|ms| u64::try_from(ms).ok()) else {
                sink.skip(SkipReason::BadDuration);
                continue;
            };
            sink.accept(Listen {
                track: TrackRef::new(artist, title),
                played_at,
                ms_played,
                source: ListenSource::Import {
                    export: ExportKind::SpotifyAccount,
                },
                canonical_id: None,
            });
        }
        Ok(())
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(entry: &str, body: &[u8]) -> Result<Vec<T>> {
    serde_json::from_slice(body).map_err(|source| {
        Error::new(
            Stage::ImportParse,
            ErrorKind::Json {
                entry: entry.to_owned(),
                source,
            },
        )
    })
}

/// The last part of the path in a zip entry.
fn file_name(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// `2023-05-01T12:34:56Z` → a UTC timestamp.
fn parse_rfc3339(raw: &str) -> Option<jiff::Timestamp> {
    raw.trim().parse::<jiff::Timestamp>().ok()
}

/// `2023-05-01 12:34` → taken as UTC.
///
/// Spotify account data has no time zone. Assuming UTC may shift a few
/// records at the year boundary; this is a deliberate trade-off and it stays
/// on record with the [`ExportKind::SpotifyAccount`] label.
fn parse_naive_minutes(raw: &str) -> Option<jiff::Timestamp> {
    let civil = jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M", raw.trim()).ok()?;
    civil
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

/// `spotify:track:70LcF31zb1H0PyJoS1Sx1r` → a provider track id.
fn track_id_from_uri(uri: &str) -> Option<ProviderTrackId> {
    let id = uri.strip_prefix("spotify:track:")?;
    (!id.is_empty()).then(|| ProviderTrackId::new(spotify_provider(), id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{MemoryArchive, import};

    const EXTENDED: &str = r#"[
      {
        "ts": "2023-05-01T12:34:56Z",
        "ms_played": 214000,
        "master_metadata_track_name": "Creep",
        "master_metadata_album_artist_name": "Radiohead",
        "master_metadata_album_album_name": "Pablo Honey",
        "spotify_track_uri": "spotify:track:70LcF31zb1H0PyJoS1Sx1r"
      },
      {
        "ts": "2023-05-01T13:00:00Z",
        "ms_played": 1800000,
        "master_metadata_track_name": null,
        "master_metadata_album_artist_name": null,
        "master_metadata_album_album_name": null,
        "spotify_track_uri": null,
        "episode_name": "A Podcast"
      },
      {
        "ts": "broken-date",
        "ms_played": 1000,
        "master_metadata_track_name": "X",
        "master_metadata_album_artist_name": "Y",
        "master_metadata_album_album_name": null,
        "spotify_track_uri": null
      }
    ]"#;

    const ACCOUNT: &str = r#"[
      {"endTime": "2023-05-01 12:34", "artistName": "Portishead", "trackName": "Roads", "msPlayed": 300000},
      {"endTime": "2023-05-01 12:40", "artistName": "", "trackName": "Untitled", "msPlayed": 1000}
    ]"#;

    #[test]
    fn extended_parses_music_and_counts_every_skip() {
        let mut archive = MemoryArchive::new("test.zip").with_entry(
            "Spotify Extended Streaming History/Streaming_History_Audio_2023.json",
            EXTENDED,
        );
        let outcome = import(&mut archive).unwrap();

        assert_eq!(outcome.summary.export, ExportKind::SpotifyExtended);
        assert_eq!(outcome.summary.records_total, 3);
        assert_eq!(outcome.summary.listens, 1);
        assert_eq!(outcome.summary.skipped[&SkipReason::NotMusic], 1);
        assert_eq!(outcome.summary.skipped[&SkipReason::BadTimestamp], 1);
        assert_eq!(outcome.summary.skipped_total(), 2);

        let listen = &outcome.listens[0];
        assert_eq!(listen.track.artist, "Radiohead");
        assert_eq!(listen.track.title, "Creep");
        assert_eq!(listen.track.album.as_deref(), Some("Pablo Honey"));
        assert_eq!(listen.ms_played, 214_000);
        assert_eq!(
            listen
                .track
                .provider_track_id
                .as_ref()
                .map(ToString::to_string),
            Some("spotify:70LcF31zb1H0PyJoS1Sx1r".to_owned())
        );
        assert_eq!(outcome.summary.with_provider_id, 1);
        assert_eq!(outcome.summary.with_isrc, 0);
    }

    #[test]
    fn account_format_is_detected_and_parsed() {
        let mut archive =
            MemoryArchive::new("account.zip").with_entry("MyData/StreamingHistory0.json", ACCOUNT);
        let outcome = import(&mut archive).unwrap();

        assert_eq!(outcome.summary.export, ExportKind::SpotifyAccount);
        assert_eq!(outcome.summary.listens, 1);
        assert_eq!(outcome.summary.skipped[&SkipReason::MissingArtist], 1);
        assert_eq!(outcome.listens[0].track.title, "Roads");
        assert_eq!(
            outcome.listens[0].played_at.to_string(),
            "2023-05-01T12:34:00Z"
        );
    }

    #[test]
    fn malformed_json_names_the_entry_and_stage() {
        let mut archive = MemoryArchive::new("broken.zip")
            .with_entry("Streaming_History_Audio_2023.json", "{ this is not json");
        let err = import(&mut archive).unwrap_err();
        assert_eq!(err.stage(), Stage::ImportParse);
        assert!(
            err.chain_text()
                .contains("Streaming_History_Audio_2023.json")
        );
    }

    #[test]
    fn track_uri_parsing_rejects_non_track_uris() {
        assert!(track_id_from_uri("spotify:episode:abc").is_none());
        assert!(track_id_from_uri("spotify:track:").is_none());
        assert!(track_id_from_uri("spotify:track:abc").is_some());
    }
}
