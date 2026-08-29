//! Spotify export ayrıştırıcıları.
//!
//! İki biçim var:
//! - **Extended streaming history** — `Streaming_History_Audio_*.json`, tüm
//!   geçmiş, parça URI'si dahil. İstenen budur.
//! - **Hesap verisi** — `StreamingHistory*.json`, yalnızca son 12 ay, URI yok.
//!
//! Not: Spotify export'ları ISRC vermez. Kimlik zinciri bu yüzden burada
//! `spotify:track:...` ile başlar ve MBID'ye [`crate::identity`] içinde çevrilir.

use serde::Deserialize;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::{ProviderId, ProviderTrackId};
use crate::import::{ExportParser, ParseSink, SkipReason};
use crate::model::{ExportKind, Listen, ListenSource, TrackRef};

/// Spotify'ın sağlayıcı adı. Çekirdek Spotify'a *bağımlı* değil; bu yalnızca
/// export'tan gelen kimliğin hangi ada ait olduğunu söyleyen bir etiket.
fn spotify_provider() -> ProviderId {
    ProviderId::new("spotify")
}

/// `Streaming_History_Audio_2023_5.json` gibi girdileri ayrıştırır.
pub(crate) struct ExtendedParser;

/// Extended history kaydı. Bilinmeyen alanlar yok sayılır; Spotify şemayı
/// zaman zaman genişletiyor ve bu yüzden hiçbir alan zorunlu tutulmuyor.
#[derive(Debug, Deserialize)]
struct ExtendedRecord {
    ts: Option<String>,
    ms_played: Option<i64>,
    master_metadata_track_name: Option<String>,
    master_metadata_album_artist_name: Option<String>,
    master_metadata_album_album_name: Option<String>,
    spotify_track_uri: Option<String>,
    /// Podcast bölümü ise doludur — bu kayıt müzik değildir.
    #[serde(default)]
    episode_name: Option<String>,
    /// Sesli kitap bölümü (2024 sonrası export'lar).
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

/// `StreamingHistory_music_0.json` / `StreamingHistory0.json` ayrıştırıcısı.
pub(crate) struct AccountParser;

#[derive(Debug, Deserialize)]
struct AccountRecord {
    /// `"2023-05-01 12:34"` — saat dilimi yok, yerel saat.
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

/// Zip girdisindeki yolun son parçası.
fn file_name(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// `2023-05-01T12:34:56Z` → UTC zaman damgası.
fn parse_rfc3339(raw: &str) -> Option<jiff::Timestamp> {
    raw.trim().parse::<jiff::Timestamp>().ok()
}

/// `2023-05-01 12:34` → UTC kabul edilir.
///
/// Spotify hesap verisinde saat dilimi yok. UTC varsaymak yıl sınırındaki
/// birkaç kaydı kaydırabilir; bu bilinçli bir takas ve
/// [`ExportKind::SpotifyAccount`] etiketiyle kayıtta durur.
fn parse_naive_minutes(raw: &str) -> Option<jiff::Timestamp> {
    let civil = jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M", raw.trim()).ok()?;
    civil
        .to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

/// `spotify:track:70LcF31zb1H0PyJoS1Sx1r` → sağlayıcı parça kimliği.
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
        "episode_name": "Bir Podcast"
      },
      {
        "ts": "bozuk-tarih",
        "ms_played": 1000,
        "master_metadata_track_name": "X",
        "master_metadata_album_artist_name": "Y",
        "master_metadata_album_album_name": null,
        "spotify_track_uri": null
      }
    ]"#;

    const ACCOUNT: &str = r#"[
      {"endTime": "2023-05-01 12:34", "artistName": "Portishead", "trackName": "Roads", "msPlayed": 300000},
      {"endTime": "2023-05-01 12:40", "artistName": "", "trackName": "Adsız", "msPlayed": 1000}
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
            MemoryArchive::new("hesap.zip").with_entry("MyData/StreamingHistory0.json", ACCOUNT);
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
        let mut archive = MemoryArchive::new("kirik.zip")
            .with_entry("Streaming_History_Audio_2023.json", "{ bu json değil");
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
