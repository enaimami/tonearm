//! Dinleme istatistikleri.
//!
//! Gruplama kanonik kimlik üzerinden yapılır; kimliği çözülmemiş kayıtlar
//! normalize anahtara düşer ve **sayılır** — kaç kaydın kimliksiz olduğu
//! raporun içinde görünür, sessizce kaybolmaz.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::identity::normalize::track_key;
use crate::ids::CanonicalId;
use crate::model::{Listen, PlayRule};

/// "Sayılan çalma" eşiği. Tanım [`crate::model`]'de; burada yalnızca
/// yeniden dışa veriliyor ki eski çağıranlar kırılmasın (D-008).
pub use crate::model::DEFAULT_MIN_MS_PLAYED;

/// İstatistik sorgusu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsQuery {
    /// Yalnızca bu takvim yılı (UTC). `None` ise tüm zamanlar.
    pub year: Option<i16>,
    /// Listelerde kaç satır döndürülecek.
    pub top: usize,
    /// Bu sürenin altındaki çalmalar atlama sayılır.
    pub min_ms_played: u64,
}

impl Default for StatsQuery {
    fn default() -> Self {
        Self {
            year: None,
            top: 10,
            min_ms_played: DEFAULT_MIN_MS_PLAYED,
        }
    }
}

impl StatsQuery {
    /// Sorgunun eşiğinden "sayılan çalma" kuralını üretir.
    ///
    /// `stats` ve `library search` bu kuralın **aynı** örneğini kullanır;
    /// eşik iki yerde ayrı yorumlanmaz (D-008).
    #[must_use]
    pub const fn play_rule(&self) -> PlayRule {
        PlayRule::new(self.min_ms_played)
    }
}

/// Bir parçanın toplamları.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackStat {
    pub artist: String,
    pub title: String,
    pub canonical_id: Option<CanonicalId>,
    pub plays: usize,
    pub ms_played: u64,
}

/// Bir sanatçının toplamları.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistStat {
    pub artist: String,
    pub plays: usize,
    pub ms_played: u64,
    pub unique_tracks: usize,
}

/// Bir albümün toplamları.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumStat {
    pub artist: String,
    pub album: String,
    pub plays: usize,
    pub ms_played: u64,
}

/// Yıl bazlı toplam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearStat {
    pub year: i16,
    pub plays: usize,
    pub ms_played: u64,
}

/// İstatistik raporu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsReport {
    pub query: StatsQuery,
    /// Sorgu kapsamına giren toplam kayıt (eşik uygulanmadan önce).
    pub listens_in_scope: usize,
    /// Eşiği geçen, yani "dinlendi" sayılan kayıtlar.
    pub plays: usize,
    /// Eşiğin altında kaldığı için atlanan kayıtlar.
    pub skipped_short: usize,
    /// Kapsam dışı kalanlar (yıl filtresi).
    pub out_of_scope: usize,
    /// Kanonik kimliği olmayan kayıt sayısı — çözümlemenin borcu.
    pub without_canonical_id: usize,
    pub total_ms_played: u64,
    pub unique_tracks: usize,
    pub unique_artists: usize,
    pub top_tracks: Vec<TrackStat>,
    pub top_artists: Vec<ArtistStat>,
    pub top_albums: Vec<AlbumStat>,
    pub by_year: Vec<YearStat>,
}

impl StatsReport {
    /// Toplam dinleme süresi, saat cinsinden.
    #[must_use]
    pub fn total_hours(&self) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "gösterim amaçlı")]
        {
            self.total_ms_played as f64 / 3_600_000.0
        }
    }

    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        recorder.set("stats.listens_in_scope", n(self.listens_in_scope));
        recorder.set("stats.plays", n(self.plays));
        recorder.set("stats.skipped_short", n(self.skipped_short));
        recorder.set("stats.out_of_scope", n(self.out_of_scope));
        recorder.set("stats.without_canonical_id", n(self.without_canonical_id));
        recorder.set("stats.unique_tracks", n(self.unique_tracks));
        recorder.set("stats.unique_artists", n(self.unique_artists));
    }
}

/// Bir parçayı gruplamak için kullanılan anahtar.
///
/// Kanonik kimlik varsa o; yoksa normalize sanatçı+başlık. İkisinin
/// karışmaması için ayrı ön ek taşırlar.
fn group_key(listen: &Listen) -> String {
    listen.canonical_id.as_ref().map_or_else(
        || {
            format!(
                "k\u{1}{}",
                track_key(&listen.track.artist, &listen.track.title)
            )
        },
        |id| format!("c\u{1}{id}"),
    )
}

fn year_of(listen: &Listen) -> i16 {
    listen.played_at.to_zoned(jiff::tz::TimeZone::UTC).year()
}

/// Dinleme kayıtlarından istatistik üretir.
///
/// Saf fonksiyon: girdi dilimi, çıktı rapor. Veri nereden geldiğiyle
/// ilgilenmez — bellek, SQLite ya da test sahtesi olabilir.
#[must_use]
pub fn compute(listens: &[Listen], query: StatsQuery) -> StatsReport {
    let rule = query.play_rule();
    let mut tracks: HashMap<String, TrackStat> = HashMap::new();
    let mut artists: HashMap<String, ArtistStat> = HashMap::new();
    let mut artist_tracks: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    let mut albums: HashMap<(String, String), AlbumStat> = HashMap::new();
    let mut years: HashMap<i16, YearStat> = HashMap::new();

    let mut listens_in_scope = 0usize;
    let mut plays = 0usize;
    let mut skipped_short = 0usize;
    let mut out_of_scope = 0usize;
    let mut without_canonical_id = 0usize;
    let mut total_ms_played = 0u64;

    for listen in listens {
        let year = year_of(listen);
        if query.year.is_some_and(|wanted| wanted != year) {
            out_of_scope += 1;
            continue;
        }
        listens_in_scope += 1;
        if !listen.counts_as_play(rule) {
            skipped_short += 1;
            continue;
        }
        plays += 1;
        total_ms_played = total_ms_played.saturating_add(listen.ms_played);
        if listen.canonical_id.is_none() {
            without_canonical_id += 1;
        }

        let key = group_key(listen);
        let track = tracks.entry(key.clone()).or_insert_with(|| TrackStat {
            artist: listen.track.artist.clone(),
            title: listen.track.title.clone(),
            canonical_id: listen.canonical_id.clone(),
            plays: 0,
            ms_played: 0,
        });
        track.plays += 1;
        track.ms_played = track.ms_played.saturating_add(listen.ms_played);

        let artist_key = crate::identity::normalize::normalize_artist(&listen.track.artist);
        let artist = artists
            .entry(artist_key.clone())
            .or_insert_with(|| ArtistStat {
                artist: listen.track.artist.clone(),
                plays: 0,
                ms_played: 0,
                unique_tracks: 0,
            });
        artist.plays += 1;
        artist.ms_played = artist.ms_played.saturating_add(listen.ms_played);
        artist_tracks.entry(artist_key).or_default().insert(key);

        if let Some(album_name) = &listen.track.album {
            let album = albums
                .entry((artist.artist.clone(), album_name.clone()))
                .or_insert_with(|| AlbumStat {
                    artist: listen.track.artist.clone(),
                    album: album_name.clone(),
                    plays: 0,
                    ms_played: 0,
                });
            album.plays += 1;
            album.ms_played = album.ms_played.saturating_add(listen.ms_played);
        }

        let year_stat = years.entry(year).or_insert(YearStat {
            year,
            plays: 0,
            ms_played: 0,
        });
        year_stat.plays += 1;
        year_stat.ms_played = year_stat.ms_played.saturating_add(listen.ms_played);
    }

    for (key, stat) in &mut artists {
        stat.unique_tracks = artist_tracks
            .get(key)
            .map_or(0, std::collections::HashSet::len);
    }

    let unique_tracks = tracks.len();
    let unique_artists = artists.len();

    let mut top_tracks: Vec<TrackStat> = tracks.into_values().collect();
    top_tracks.sort_by(|a, b| {
        b.plays
            .cmp(&a.plays)
            .then_with(|| b.ms_played.cmp(&a.ms_played))
            .then_with(|| a.title.cmp(&b.title))
    });
    top_tracks.truncate(query.top);

    let mut top_artists: Vec<ArtistStat> = artists.into_values().collect();
    top_artists.sort_by(|a, b| {
        b.plays
            .cmp(&a.plays)
            .then_with(|| b.ms_played.cmp(&a.ms_played))
            .then_with(|| a.artist.cmp(&b.artist))
    });
    top_artists.truncate(query.top);

    let mut top_albums: Vec<AlbumStat> = albums.into_values().collect();
    top_albums.sort_by(|a, b| {
        b.plays
            .cmp(&a.plays)
            .then_with(|| b.ms_played.cmp(&a.ms_played))
            .then_with(|| a.album.cmp(&b.album))
    });
    top_albums.truncate(query.top);

    let mut by_year: Vec<YearStat> = years.into_values().collect();
    by_year.sort_by_key(|stat| stat.year);

    StatsReport {
        query,
        listens_in_scope,
        plays,
        skipped_short,
        out_of_scope,
        without_canonical_id,
        total_ms_played,
        unique_tracks,
        unique_artists,
        top_tracks,
        top_artists,
        top_albums,
        by_year,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExportKind, ListenSource, TrackRef};

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

    fn sample() -> Vec<Listen> {
        vec![
            listen("Radiohead", "Creep", "2023-01-01T00:00:00Z", 200_000),
            listen(
                "Radiohead",
                "Creep (Remastered)",
                "2023-02-01T00:00:00Z",
                200_000,
            ),
            listen("Radiohead", "Karma Police", "2023-03-01T00:00:00Z", 260_000),
            listen("Portishead", "Roads", "2024-01-01T00:00:00Z", 300_000),
            listen("Portishead", "Roads", "2024-01-02T00:00:00Z", 5_000),
        ]
    }

    #[test]
    fn short_plays_are_counted_not_dropped_silently() {
        let report = compute(&sample(), StatsQuery::default());
        assert_eq!(report.listens_in_scope, 5);
        assert_eq!(report.plays, 4);
        assert_eq!(report.skipped_short, 1);
        assert_eq!(report.plays + report.skipped_short, report.listens_in_scope);
    }

    #[test]
    fn same_track_with_different_titles_groups_by_normalized_key() {
        let report = compute(&sample(), StatsQuery::default());
        let creep = report
            .top_tracks
            .iter()
            .find(|t| t.title.starts_with("Creep"))
            .expect("Creep raporda olmalı");
        assert_eq!(creep.plays, 2, "Remastered ayrı parça sayılmamalı");
        assert_eq!(report.unique_tracks, 3);
    }

    #[test]
    fn canonical_id_overrides_text_grouping() {
        let mut listens = sample();
        let id = CanonicalId::from_local_key("elle-verilmis");
        listens[0].canonical_id = Some(id.clone());
        listens[2].canonical_id = Some(id);
        let report = compute(&listens, StatsQuery::default());
        let top = &report.top_tracks[0];
        assert_eq!(top.plays, 2, "aynı kanonik kimlik tek satırda toplanmalı");
        assert_eq!(report.without_canonical_id, 2);
    }

    #[test]
    fn year_filter_reports_what_it_excluded() {
        let query = StatsQuery {
            year: Some(2023),
            ..StatsQuery::default()
        };
        let report = compute(&sample(), query);
        assert_eq!(report.listens_in_scope, 3);
        assert_eq!(report.out_of_scope, 2);
        assert_eq!(report.by_year.len(), 1);
        assert_eq!(report.by_year[0].year, 2023);
    }

    #[test]
    fn top_n_is_respected_and_ordered_by_plays() {
        let query = StatsQuery {
            top: 1,
            ..StatsQuery::default()
        };
        let report = compute(&sample(), query);
        assert_eq!(report.top_artists.len(), 1);
        assert_eq!(report.top_artists[0].artist, "Radiohead");
        assert_eq!(report.top_artists[0].plays, 3);
        assert_eq!(report.top_artists[0].unique_tracks, 2);
    }

    #[test]
    fn empty_input_yields_an_empty_but_valid_report() {
        let report = compute(&[], StatsQuery::default());
        assert_eq!(report.plays, 0);
        assert_eq!(report.total_hours(), 0.0);
        assert!(report.top_tracks.is_empty());
    }
}
