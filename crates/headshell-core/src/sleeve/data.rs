//! Kartın girdi verisi: `StatsReport` + ham dinlemelerden türetilen
//! kart-özel toplamlar (ilk dinleme, arşiv kapsamı, keşifler).
//!
//! Hesap saf ve yereldir: ağaç yok, girdi dilimi + rapor → kart verisi.

use serde::{Deserialize, Serialize};

use crate::identity::normalize::normalize_artist;
use crate::model::Listen;
use crate::stats::{AlbumStat, ArtistStat, StatsReport, TrackStat, YearStat};

/// Kartın bütün göstereceği sayı ve isimler.
///
/// SVG üretimi [`super::svg`] yalnızca bunu okur — veri toplama ve çizim
/// ayrı katmanlar, GUI ileride aynı veriyle kendi çizimini yapabilir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleeveData {
    /// Kartın dönemi: `Some(2026)` → yıl kartı, `None` → tüm zamanlar.
    pub year: Option<i16>,
    pub total_ms_played: u64,
    pub plays: usize,
    pub unique_tracks: usize,
    pub unique_artists: usize,
    pub top_artist: Option<ArtistStat>,
    pub top_track: Option<TrackStat>,
    pub top_album: Option<AlbumStat>,
    /// Kapsam içindeki ilk dinleme anı. Boş kütüphanede `None`.
    pub first_listen: Option<jiff::Timestamp>,
    /// Tüm arşivin (yıl filtresinden bağımsız) ilk ve son dinleme yılı.
    /// "8 yıllık arşiv" anlatısının kaynağı — Sleeve'ın, sağlayıcıların
    /// 12 aylık hafızasından (Spotify Wrapped®) ayrıldığı yer.
    pub archive_start_year: Option<i16>,
    pub archive_end_year: Option<i16>,
    /// Yıllara göre çalma sayıları (kapsam içinde). Tek yıl varsa zaman
    /// çizelgesi anlamsızdır; çizim katmanı bölümü kendisi atlar.
    pub by_year: Vec<YearStat>,
    /// Keşif çizelgesi: yıl kartında "bu yıl ilk kez dinlediğin" sanatçılar
    /// (en çok dinlenene göre); tüm zamanlar kartında en çok dinlenen
    /// sanatçıların ilk dinleme yılları (kronolojik).
    pub discoveries: Vec<Discovery>,
}

/// Bir sanatçının keşfi: ilk dinleme yılı + o kapsamda kaç çalması olduğu.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Discovery {
    pub artist: String,
    /// Sanatçın **tüm arşivdeki** ilk dinleme yılı.
    pub first_year: i16,
    /// Sorgu kapsamındaki sayılan çalma sayısı.
    pub plays_in_scope: usize,
}

/// Kapsam içi (yıl filtresi + çalma kuralı) dinlemeleri süzer.
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

/// Kart verisini üretir.
///
/// `report` kapsamın istatistikleri, `listens` ise **filtresiz** tüm dinlemeler:
/// keşif ve arşiv kapsamı yalnızca tüm arşive bakarak doğru hesaplanır.
#[must_use]
pub fn card_data(report: &StatsReport, listens: &[Listen]) -> SleeveData {
    // Tüm arşivin kapsamı — yıl filtresinden bağımsız.
    let archive_start_year = listens.iter().map(year_of).min();
    let archive_end_year = listens.iter().map(year_of).max();

    // Kapsam içi ilk dinleme (eşik gözetmeksizin: "o an" dediğimiz andır).
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

    // Kapsam içi sanatçı tablosu ve tüm arşivdeki ilk dinleme yılları.
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
        // Yıl kartı: bu yıl **keşfedilenler** — ilk dinleme yılı sorgu yılına eşit.
        Some(year) => {
            discoveries.retain(|d| d.first_year == year);
            discoveries.sort_by(|a, b| {
                b.plays_in_scope
                    .cmp(&a.plays_in_scope)
                    .then_with(|| a.artist.cmp(&b.artist))
            });
        }
        // Tüm zamanlar: en çok dinlenenler, kronolojik olarak.
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

/// Kartta gösterilecek en fazla keşif satırı.
pub const TOP_DISCOVERIES: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExportKind, ListenSource, TrackRef};
    use crate::stats::{self, StatsQuery};

    fn listen(artist: &str, title: &str, ts: &str, ms: u64) -> Listen {
        Listen {
            track: TrackRef::new(artist, title).with_album(Some(format!("{title} albümü"))),
            played_at: ts.parse().expect("test zaman damgası geçerli"),
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
        // Kapsam içi ilk dinleme 2024'ün içinden gelir.
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
        // 2021'de dinlenen tek sanatçı Portishead ve ilk dinlemesi de 2021: keşif.
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
        // Radiohead 2019'dan beri var; 2026 kartında keşif listesinde olmamalı.
        // 2026'da ilk kez dinlenen Sault kalmalı.
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
