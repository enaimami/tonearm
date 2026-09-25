//! The card's input data: `StatsReport` + card-specific totals derived from
//! raw listens (the first listen, the archive span, discoveries).
//!
//! The calculation is pure and local: no tree, an input slice + a report →
//! card data.

use serde::{Deserialize, Serialize};

use crate::identity::normalize::normalize_artist;
use crate::model::Listen;
use crate::stats::{AlbumStat, ArtistStat, StatsReport, TrackStat, YearStat};

/// All the numbers and names the card will show.
///
/// SVG generation [`super::svg`] reads only this — collecting data and
/// drawing are separate layers, and the GUI can later do its own drawing from
/// the same data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleeveData {
    /// The card's period: `Some(2026)` → a year card, `None` → all time.
    pub year: Option<i16>,
    pub total_ms_played: u64,
    pub plays: usize,
    pub unique_tracks: usize,
    pub unique_artists: usize,
    pub top_artist: Option<ArtistStat>,
    pub top_track: Option<TrackStat>,
    pub top_album: Option<AlbumStat>,
    /// The moment of the first listen within scope. `None` for an empty library.
    pub first_listen: Option<jiff::Timestamp>,
    /// The first and last listening year of the whole archive (independent of the
    /// year filter). The source of the "an 8-year archive" story — where Sleeve
    /// parts ways with the providers' 12-month memory (Spotify Wrapped®).
    pub archive_start_year: Option<i16>,
    pub archive_end_year: Option<i16>,
    /// Play counts by year (within scope). If there is only one year a timeline
    /// is meaningless; the drawing layer skips that section itself.
    pub by_year: Vec<YearStat>,
    /// The discovery chart: on a year card, the artists "you listened to for the
    /// first time this year" (by most listened); on the all-time card, the first
    /// listening years of the most listened artists (chronological).
    pub discoveries: Vec<Discovery>,
}

/// An artist's discovery: the first listening year + how many plays it has in
/// that scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Discovery {
    pub artist: String,
    /// The artist's first listening year **in the whole archive**.
    pub first_year: i16,
    /// The number of counted plays within the query's scope.
    pub plays_in_scope: usize,
}

/// Filters the listens within scope (the year filter + the play rule).
fn in_scope<'a>(listens: &'a [Listen], report: &StatsReport) -> impl Iterator<Item = &'a Listen> {
    let rule = report.query.play_rule();
    listens.iter().filter(move |listen| {
        let year = listen.played_at.to_zoned(jiff::tz::TimeZone::UTC).year();
        report.query.year.is_none_or(|wanted| wanted == year) && listen.counts_as_play(rule)
    })
}

fn year_of(listen: &Listen) -> i16 {
    listen.played_at.to_zoned(jiff::tz::TimeZone::UTC).year()
}

/// Produces the card data.
///
/// `report` is the scope's statistics, while `listens` is **all** listens,
/// unfiltered: discoveries and the archive span are only computed correctly
/// by looking at the whole archive.
#[must_use]
pub fn card_data(report: &StatsReport, listens: &[Listen]) -> SleeveData {
    // The span of the whole archive — independent of the year filter.
    let archive_start_year = listens.iter().map(year_of).min();
    let archive_end_year = listens.iter().map(year_of).max();

    // The first listen within scope (regardless of the threshold: it is the
    // moment we call "that moment").
    let first_listen = listens
        .iter()
        .filter(|listen| {
            report
                .query
                .year
                .is_none_or(|wanted| wanted == year_of(listen))
        })
        .map(|listen| listen.played_at)
        .min();

    // The artist table within scope and the first listening years in the whole
    // archive.
    let mut scoped: std::collections::HashMap<String, (String, usize)> =
        std::collections::HashMap::new();
    for listen in in_scope(listens, report) {
        let key = normalize_artist(&listen.track.artist);
        let entry = scoped
            .entry(key)
            .or_insert_with(|| (listen.track.artist.clone(), 0));
        entry.1 += 1;
    }
    let mut first_years: std::collections::HashMap<String, i16> = std::collections::HashMap::new();
    for listen in listens {
        let key = normalize_artist(&listen.track.artist);
        let year = year_of(listen);
        first_years
            .entry(key)
            .and_modify(|current| *current = (*current).min(year))
            .or_insert(year);
    }

    let mut discoveries: Vec<Discovery> = scoped
        .into_iter()
        .map(|(key, (artist, plays_in_scope))| Discovery {
            artist,
            first_year: first_years.get(&key).copied().unwrap_or_default(),
            plays_in_scope,
        })
        .collect();

    match report.query.year {
        // A year card: those **discovered** this year — the first listening year
        // equals the query's year.
        Some(year) => {
            discoveries.retain(|d| d.first_year == year);
            discoveries.sort_by(|a, b| {
                b.plays_in_scope
                    .cmp(&a.plays_in_scope)
                    .then_with(|| a.artist.cmp(&b.artist))
            });
        }
        // All time: the most listened, in chronological order.
        None => {
            discoveries.sort_by(|a, b| {
                b.plays_in_scope
                    .cmp(&a.plays_in_scope)
                    .then_with(|| a.artist.cmp(&b.artist))
            });
            discoveries.truncate(TOP_DISCOVERIES);
            discoveries.sort_by_key(|d| d.first_year);
        }
    }
    discoveries.truncate(TOP_DISCOVERIES);

    SleeveData {
        year: report.query.year,
        total_ms_played: report.total_ms_played,
        plays: report.plays,
        unique_tracks: report.unique_tracks,
        unique_artists: report.unique_artists,
        top_artist: report.top_artists.first().cloned(),
        top_track: report.top_tracks.first().cloned(),
        top_album: report.top_albums.first().cloned(),
        first_listen,
        archive_start_year,
        archive_end_year,
        by_year: report.by_year.clone(),
        discoveries,
    }
}

/// The most discovery rows to show on the card.
pub const TOP_DISCOVERIES: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExportKind, ListenSource, TrackRef};
    use crate::stats::{self, StatsQuery};

    fn listen(artist: &str, title: &str, ts: &str, ms: u64) -> Listen {
        Listen {
            track: TrackRef::new(artist, title).with_album(Some(format!("{title} album"))),
            played_at: ts.parse().expect("the test timestamp is valid"),
            ms_played: ms,
            source: ListenSource::Import {
                export: ExportKind::SpotifyExtended,
            },
            canonical_id: None,
        }
    }

    fn archive() -> Vec<Listen> {
        vec![
            listen("Radiohead", "Creep", "2019-06-01T10:00:00Z", 200_000),
            listen("Radiohead", "Karma Police", "2019-08-01T10:00:00Z", 260_000),
            listen("Portishead", "Roads", "2021-02-01T10:00:00Z", 300_000),
            listen("Portishead", "Glory Box", "2024-05-01T10:00:00Z", 290_000),
            listen("Sault", "9", "2026-03-01T10:00:00Z", 210_000),
        ]
    }

    #[test]
    fn archive_span_ignores_the_year_filter() {
        let report = stats::compute(
            &archive(),
            StatsQuery {
                year: Some(2024),
                ..StatsQuery::default()
            },
        );
        let data = card_data(&report, &archive());
        assert_eq!(data.archive_start_year, Some(2019));
        assert_eq!(data.archive_end_year, Some(2026));
        // The first listen within scope comes from inside 2024.
        assert_eq!(
            data.first_listen
                .as_ref()
                .map(|t| t.to_string().starts_with("2024-05-01")),
            Some(true)
        );
    }

    #[test]
    fn year_card_lists_artists_discovered_that_year() {
        let report = stats::compute(
            &archive(),
            StatsQuery {
                year: Some(2021),
                ..StatsQuery::default()
            },
        );
        let data = card_data(&report, &archive());
        // The only artist listened to in 2021 is Portishead, and its first listen is
        // in 2021 too: a discovery.
        let names: Vec<&str> = data.discoveries.iter().map(|d| d.artist.as_str()).collect();
        assert_eq!(names, vec!["Portishead"]);
        assert_eq!(data.discoveries[0].plays_in_scope, 1);
    }

    #[test]
    fn an_old_artist_played_this_year_is_not_a_discovery() {
        let mut listens = archive();
        listens.push(listen(
            "Radiohead",
            "Creep",
            "2026-07-01T10:00:00Z",
            200_000,
        ));
        let report = stats::compute(
            &listens,
            StatsQuery {
                year: Some(2026),
                ..StatsQuery::default()
            },
        );
        let data = card_data(&report, &listens);
        // Radiohead has been around since 2019; it must not be in the discovery list
        // on the 2026 card. Sault, first listened to in 2026, must remain.
        assert!(data.discoveries.iter().all(|d| d.artist != "Radiohead"));
        assert!(data.discoveries.iter().any(|d| d.artist == "Sault"));
    }

    #[test]
    fn all_time_card_orders_discoveries_chronologically() {
        let report = stats::compute(&archive(), StatsQuery::default());
        let data = card_data(&report, &archive());
        let years: Vec<i16> = data.discoveries.iter().map(|d| d.first_year).collect();
        assert_eq!(years, vec![2019, 2021, 2026]);
        assert!(data.discoveries.iter().any(|d| d.artist == "Radiohead"));
    }

    #[test]
    fn empty_library_yields_none_fields_not_defaults() {
        let report = stats::compute(&[], StatsQuery::default());
        let data = card_data(&report, &[]);
        assert_eq!(data.first_listen, None);
        assert_eq!(data.archive_start_year, None);
        assert_eq!(data.top_artist, None);
        assert!(data.discoveries.is_empty());
    }
}
