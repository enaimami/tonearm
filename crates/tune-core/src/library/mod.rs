//! Yerel kütüphane: SQLite + FTS5.
//!
//! Depoya erişim [`ListenStore`] trait'i arkasında; testler sahte depo
//! kullanabilir, çekirdeğin geri kalanı SQLite'ı hiç görmez.

mod schema;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::identity::normalize::track_key;
use crate::identity::{Resolution, ResolveMethod};
use crate::ids::{CanonicalId, Isrc, ProviderId, ProviderTrackId};
use crate::model::{ExportKind, Listen, ListenSource, PlayRule, TrackRef};

/// Dinlemeleri yazma sonucunun özeti.
///
/// Aynı export'u iki kez içe aktarmak sayıları şişirmemeli; kaçının zaten
/// var olduğu burada görünür.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteSummary {
    /// Yazılmak üzere verilen kayıt sayısı.
    pub offered: usize,
    /// Yeni eklenen dinleme sayısı.
    pub inserted: usize,
    /// Zaten var olduğu için atlanan (aynı parça + zaman + süre).
    pub duplicates: usize,
    /// Yeni oluşturulan parça satırı sayısı.
    pub new_tracks: usize,
}

impl WriteSummary {
    /// Sayaçları tanı kaydediciye aktarır.
    pub fn record_into(&self, recorder: &mut crate::diag::Recorder) {
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        recorder.set("library.offered", n(self.offered));
        recorder.set("library.inserted", n(self.inserted));
        recorder.set("library.duplicates", n(self.duplicates));
        recorder.set("library.new_tracks", n(self.new_tracks));
    }
}

/// Aramadan dönen satır.
///
/// Sayı alanı bilerek `play_count` adını taşıyor: [`PlayRule`]'u geçen
/// çalmalar. Ham olay sayısı burada değil, [`SearchOutcome::listen_events`]
/// içinde — kullanıcıya iki farklı "çalma" göstermemek için (D-008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub canonical_id: Option<CanonicalId>,
    /// [`PlayRule`]'u geçen çalma sayısı — `stats` ile aynı hesap.
    pub play_count: usize,
    /// Yalnızca sayılan çalmaların toplam süresi.
    pub ms_played: u64,
}

/// Bir aramanın tam sonucu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    /// Eşleşen parçaların **ham** dinleme olayı sayısı (eşik uygulanmadan).
    ///
    /// Kullanıcı yüzeyine çıkmaz; `diag` sayaçlarına yazılır. `play_count` ile
    /// arasındaki fark "kaç çalma eşiğin altında kaldı" sorusunun cevabıdır.
    pub listen_events: usize,
}

/// Dinleme deposu. SQLite bunun tek gerçek uygulaması, testler sahte kullanır.
pub trait ListenStore {
    /// Dinlemeleri yazar; tekrarları atlar.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn insert_listens(&mut self, listens: &[Listen]) -> Result<WriteSummary>;

    /// Bütün dinlemeleri okur (zaman sırasına göre).
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn all_listens(&self) -> Result<Vec<Listen>>;

    /// Tam metin arama.
    ///
    /// `rule` ile hangi dinlemelerin "çalma" sayıldığı belirlenir; `stats`
    /// aynı kuralı kullanır (D-008).
    ///
    /// # Errors
    /// Sorgu geçersizse ya da veritabanı hatasında.
    fn search(&self, query: &str, limit: usize, rule: PlayRule) -> Result<SearchOutcome>;

    /// Bir parçanın çözümleme sonucunu kaydeder.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn set_resolution(&mut self, norm_key: &str, resolution: &Resolution) -> Result<()>;
}

/// Sağlayıcı kataloğundaki bir parça (kalıcı indeks satırı).
///
/// [`SearchHit`]'ten farklı: o "ne dinledin", bu "ne çalabilirsin".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogTrack {
    pub id: ProviderTrackId,
    pub track: TrackRef,
    /// Üstveri etiketlerden mi geldi (yoksa dosya adından türetildi).
    pub from_tags: bool,
    /// Dosyanın son değişme zamanı (ms) — değişmediyse yeniden okunmaz.
    pub mtime_ms: Option<i64>,
}

/// Kalıcı katalog yazımının özeti.
///
/// K9: kaç satır eklendi, kaçı güncellendi, kaçı **silindi** (dosya artık
/// yok) ayrı ayrı görünür.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogWriteSummary {
    pub inserted: usize,
    pub updated: usize,
    /// Kaynakta artık bulunmayan, bu yüzden katalogdan düşen satırlar.
    pub removed: usize,
    pub unchanged: usize,
}

/// Sağlayıcı kataloğunun kalıcı deposu.
///
/// `ListenStore`'dan ayrı bir trait: dinleme geçmişi ile çalınabilir katalog
/// farklı ömürlere sahip. Geçmiş asla silinmez; katalog kaynağı yansıtır.
pub trait CatalogStore {
    /// Bir sağlayıcının kataloğunu **tamamen değiştirir**.
    ///
    /// Verilen listede olmayan satırlar silinir — katalog kaynağın aynası
    /// olmalı, yoksa silinmiş dosyalar aramada görünmeye devam eder.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn replace_catalog(
        &mut self,
        provider: &ProviderId,
        tracks: &[CatalogTrack],
    ) -> Result<CatalogWriteSummary>;

    /// Katalogda tam metin arama.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn search_catalog(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>>;

    /// Bir sağlayıcının katalogdaki parça sayısı.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn catalog_len(&self, provider: &ProviderId) -> Result<usize>;

    /// Bir sağlayıcı kimliğiyle tek bir katalog satırı.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn catalog_get(&self, id: &ProviderTrackId) -> Result<Option<CatalogTrack>>;

    /// Bir sağlayıcının bilinen dosya damgaları: `provider_ref → mtime_ms`.
    ///
    /// Tarama bunu okuyup değişmemiş dosyaların etiketlerini yeniden
    /// okumaz — büyük kütüphanede taramanın pahalı kısmı budur.
    ///
    /// # Errors
    /// Veritabanı hatalarında.
    fn catalog_stamps(
        &self,
        provider: &ProviderId,
    ) -> Result<std::collections::HashMap<String, i64>>;
}

/// SQLite tabanlı kütüphane.
pub struct SqliteLibrary {
    conn: rusqlite::Connection,
    path: PathBuf,
}

fn db_err(stage: Stage) -> impl Fn(rusqlite::Error) -> Error {
    move |source| Error::new(stage, ErrorKind::Database { source })
}

/// [`PlayRule`]'un SQLite karşılığı.
///
/// Kuralın **tanımı** [`PlayRule::counts`] içinde; burası onu tek bir yerde
/// SQL'e çeviriyor — sorgunun içine elle eşik yazılmıyor. İkisinin ayrışması
/// D-008'deki hatanın ta kendisiydi, bu yüzden eşlik
/// `sql_play_rule_agrees_with_rust` testiyle kilitli.
///
/// Tablo takma adları sabit: `l` = `listens`, `t` = `tracks`.
fn play_predicate_sql(rule: PlayRule) -> String {
    format!(
        "(l.ms_played >= {min} \
          OR (t.duration_ms IS NOT NULL AND t.duration_ms > 0 \
              AND l.ms_played * 2 >= t.duration_ms))",
        min = rule.min_ms_played
    )
}

impl SqliteLibrary {
    /// Veritabanını açar (yoksa oluşturur) ve şemayı günceller.
    ///
    /// # Errors
    /// Dosya açılamazsa ya da göç başarısız olursa.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|source| crate::error::io_err(Stage::LibraryOpen, parent, source))?;
            }
        }
        let conn = rusqlite::Connection::open(&path).map_err(db_err(Stage::LibraryOpen))?;
        let mut library = Self { conn, path };
        library.configure()?;
        library.migrate()?;
        Ok(library)
    }

    /// Bellekte geçici kütüphane — testler ve `--dry-run` için.
    ///
    /// # Errors
    /// Şema kurulamazsa.
    pub fn open_in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory().map_err(db_err(Stage::LibraryOpen))?;
        let mut library = Self {
            conn,
            path: PathBuf::from(":memory:"),
        };
        library.configure()?;
        library.migrate()?;
        Ok(library)
    }

    /// Veritabanı dosyasının yolu.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn configure(&self) -> Result<()> {
        self.conn
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err(Stage::LibraryOpen))?;
        self.conn
            .pragma_update(None, "foreign_keys", true)
            .map_err(db_err(Stage::LibraryOpen))?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        let current: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(db_err(Stage::LibraryOpen))?;
        let current = usize::try_from(current).unwrap_or(0);

        for (index, migration) in schema::MIGRATIONS.iter().enumerate().skip(current) {
            let version = index + 1;
            tracing::info!(version, "şema göçü uygulanıyor");
            self.conn
                .execute_batch(migration)
                .map_err(db_err(Stage::LibraryOpen))?;
            self.conn
                .pragma_update(None, "user_version", i64::try_from(version).unwrap_or(0))
                .map_err(db_err(Stage::LibraryOpen))?;
        }
        Ok(())
    }

    /// Parça satırını bulur ya da oluşturur; `(id, yeni_mi)` döndürür.
    fn upsert_track(tx: &rusqlite::Transaction<'_>, track: &TrackRef) -> Result<(i64, bool)> {
        let key = track_key(&track.artist, &track.title);
        // SQLite tam sayıları işaretli; süreyi i64'e daraltıyoruz.
        let duration_ms = track.duration_ms.and_then(|ms| i64::try_from(ms).ok());
        let existing: Option<i64> = tx
            .query_row("SELECT id FROM tracks WHERE norm_key = ?1", [&key], |row| {
                row.get(0)
            })
            .optional_row()?;

        if let Some(id) = existing {
            // Daha zengin üstveri geldiyse boş alanları doldur; var olanı ezme.
            tx.execute(
                "UPDATE tracks SET
                     album             = COALESCE(album, ?2),
                     duration_ms       = COALESCE(duration_ms, ?3),
                     isrc              = COALESCE(isrc, ?4),
                     provider          = COALESCE(provider, ?5),
                     provider_track_id = COALESCE(provider_track_id, ?6)
                 WHERE id = ?1",
                rusqlite::params![
                    id,
                    track.album,
                    duration_ms,
                    track.isrc.as_ref().map(Isrc::as_str),
                    track
                        .provider_track_id
                        .as_ref()
                        .map(|p| p.provider.as_str()),
                    track.provider_track_id.as_ref().map(|p| p.id.as_str()),
                ],
            )
            .map_err(db_err(Stage::LibraryWrite))?;
            return Ok((id, false));
        }

        tx.execute(
            "INSERT INTO tracks
                 (norm_key, artist, title, album, duration_ms, isrc, provider, provider_track_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                key,
                track.artist,
                track.title,
                track.album,
                duration_ms,
                track.isrc.as_ref().map(Isrc::as_str),
                track
                    .provider_track_id
                    .as_ref()
                    .map(|p| p.provider.as_str()),
                track.provider_track_id.as_ref().map(|p| p.id.as_str()),
            ],
        )
        .map_err(db_err(Stage::LibraryWrite))?;
        Ok((tx.last_insert_rowid(), true))
    }
}

/// `query_row`'un "satır yok" hâlini hataya çevirmeden `Option`'a indirger.
trait OptionalRow<T> {
    fn optional_row(self) -> Result<Option<T>>;
}

impl<T> OptionalRow<T> for std::result::Result<T, rusqlite::Error> {
    fn optional_row(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(source) => Err(Error::new(
                Stage::LibraryQuery,
                ErrorKind::Database { source },
            )),
        }
    }
}

impl ListenStore for SqliteLibrary {
    fn insert_listens(&mut self, listens: &[Listen]) -> Result<WriteSummary> {
        let tx = self
            .conn
            .transaction()
            .map_err(db_err(Stage::LibraryWrite))?;
        let mut summary = WriteSummary {
            offered: listens.len(),
            ..WriteSummary::default()
        };

        for listen in listens {
            let (track_id, is_new) = Self::upsert_track(&tx, &listen.track)?;
            if is_new {
                summary.new_tracks += 1;
            }
            let (source_kind, source_ref) = match &listen.source {
                ListenSource::Import { export } => ("import", export.as_str().to_owned()),
                ListenSource::Playback { provider } => ("playback", provider.as_str().to_owned()),
            };
            let changed = tx
                .execute(
                    "INSERT OR IGNORE INTO listens
                         (track_id, played_at, ms_played, source_kind, source_ref)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        track_id,
                        listen.played_at.as_millisecond(),
                        i64::try_from(listen.ms_played).unwrap_or(i64::MAX),
                        source_kind,
                        source_ref,
                    ],
                )
                .map_err(db_err(Stage::LibraryWrite))?;
            if changed == 0 {
                summary.duplicates += 1;
            } else {
                summary.inserted += 1;
            }
        }

        tx.commit().map_err(db_err(Stage::LibraryWrite))?;
        tracing::info!(
            inserted = summary.inserted,
            duplicates = summary.duplicates,
            "kütüphaneye yazıldı"
        );
        Ok(summary)
    }

    fn all_listens(&self) -> Result<Vec<Listen>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.artist, t.title, t.album, t.duration_ms, t.isrc,
                        t.provider, t.provider_track_id, t.canonical_id,
                        l.played_at, l.ms_played, l.source_kind, l.source_ref
                 FROM listens l JOIN tracks t ON t.id = l.track_id
                 ORDER BY l.played_at",
            )
            .map_err(db_err(Stage::LibraryQuery))?;

        let rows = stmt
            .query_map([], |row| {
                let artist: String = row.get(0)?;
                let title: String = row.get(1)?;
                let album: Option<String> = row.get(2)?;
                let duration_ms: Option<i64> = row.get(3)?;
                let isrc: Option<String> = row.get(4)?;
                let provider: Option<String> = row.get(5)?;
                let provider_track_id: Option<String> = row.get(6)?;
                let canonical_id: Option<String> = row.get(7)?;
                let played_at_ms: i64 = row.get(8)?;
                let ms_played: i64 = row.get(9)?;
                let source_kind: String = row.get(10)?;
                let source_ref: Option<String> = row.get(11)?;
                Ok((
                    artist,
                    title,
                    album,
                    duration_ms,
                    isrc,
                    provider,
                    provider_track_id,
                    canonical_id,
                    played_at_ms,
                    ms_played,
                    source_kind,
                    source_ref,
                ))
            })
            .map_err(db_err(Stage::LibraryQuery))?;

        let mut listens = Vec::new();
        for row in rows {
            let (
                artist,
                title,
                album,
                duration_ms,
                isrc,
                provider,
                provider_track_id,
                canonical_id,
                played_at_ms,
                ms_played,
                source_kind,
                source_ref,
            ) = row.map_err(db_err(Stage::LibraryQuery))?;

            let played_at = jiff::Timestamp::from_millisecond(played_at_ms).map_err(|err| {
                Error::new(
                    Stage::LibraryQuery,
                    ErrorKind::InvalidInput {
                        detail: format!("depodaki zaman damgası okunamadı ({played_at_ms}): {err}"),
                    },
                )
            })?;

            let track = TrackRef::new(artist, title)
                .with_album(album)
                .with_duration_ms(duration_ms.and_then(|ms| u64::try_from(ms).ok()))
                .with_isrc(isrc.as_deref().and_then(Isrc::parse))
                .with_provider_track_id(match (provider, provider_track_id) {
                    (Some(p), Some(id)) => Some(ProviderTrackId::new(ProviderId::new(p), id)),
                    _ => None,
                });

            listens.push(Listen {
                track,
                played_at,
                ms_played: u64::try_from(ms_played).unwrap_or(0),
                source: decode_source(&source_kind, source_ref.as_deref()),
                canonical_id: canonical_id.map(CanonicalId::from_stored),
            });
        }
        Ok(listens)
    }

    fn search(&self, query: &str, limit: usize, rule: PlayRule) -> Result<SearchOutcome> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(Error::new(
                Stage::LibraryQuery,
                ErrorKind::InvalidInput {
                    detail: "arama sorgusu boş".to_owned(),
                },
            ));
        }

        let counted = play_predicate_sql(rule);
        let sql = format!(
            // bm25() toplama (GROUP BY) bağlamında çağrılamaz. MATERIALIZED
            // olmadan SQLite alt sorguyu dış sorguya düzleştirip aynı hatayı
            // veriyor, bu yüzden eşleşmeler önce ayrıca hesaplanıyor.
            "WITH matches AS MATERIALIZED (
                 SELECT rowid AS track_id, bm25(tracks_fts) AS score
                 FROM tracks_fts WHERE tracks_fts MATCH ?1
             )
             SELECT t.artist, t.title, t.album, t.canonical_id,
                    COALESCE(SUM(CASE WHEN {counted} THEN 1 ELSE 0 END), 0) AS play_count,
                    COALESCE(SUM(CASE WHEN {counted} THEN l.ms_played ELSE 0 END), 0),
                    COUNT(l.id) AS listen_events
             FROM matches m
             JOIN tracks t ON t.id = m.track_id
             LEFT JOIN listens l ON l.track_id = t.id
             GROUP BY t.id
             ORDER BY play_count DESC, listen_events DESC, MIN(m.score)
             LIMIT ?2"
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(db_err(Stage::LibraryQuery))?;

        let pattern = fts_pattern(trimmed);
        let rows = stmt
            .query_map(
                rusqlite::params![pattern, i64::try_from(limit).unwrap_or(i64::MAX)],
                |row| {
                    let hit = SearchHit {
                        artist: row.get(0)?,
                        title: row.get(1)?,
                        album: row.get(2)?,
                        canonical_id: row
                            .get::<_, Option<String>>(3)?
                            .map(CanonicalId::from_stored),
                        play_count: usize::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                        ms_played: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                    };
                    let listen_events = usize::try_from(row.get::<_, i64>(6)?).unwrap_or(0);
                    Ok((hit, listen_events))
                },
            )
            .map_err(db_err(Stage::LibraryQuery))?;

        let mut outcome = SearchOutcome::default();
        for row in rows {
            let (hit, listen_events) = row.map_err(db_err(Stage::LibraryQuery))?;
            outcome.listen_events += listen_events;
            outcome.hits.push(hit);
        }
        Ok(outcome)
    }

    fn set_resolution(&mut self, norm_key: &str, resolution: &Resolution) -> Result<()> {
        self.conn
            .execute(
                "UPDATE tracks
                 SET canonical_id = ?2, resolve_method = ?3, resolve_confidence = ?4
                 WHERE norm_key = ?1",
                rusqlite::params![
                    norm_key,
                    resolution.canonical_id.as_str(),
                    resolution.method.as_str(),
                    resolution.confidence,
                ],
            )
            .map_err(db_err(Stage::LibraryWrite))?;
        Ok(())
    }
}

fn decode_source(kind: &str, reference: Option<&str>) -> ListenSource {
    match (kind, reference) {
        ("playback", Some(provider)) => ListenSource::Playback {
            provider: ProviderId::new(provider),
        },
        (_, Some("spotify_account")) => ListenSource::Import {
            export: ExportKind::SpotifyAccount,
        },
        (_, Some("apple_music")) => ListenSource::Import {
            export: ExportKind::AppleMusic,
        },
        (_, Some("google_takeout")) => ListenSource::Import {
            export: ExportKind::GoogleTakeout,
        },
        _ => ListenSource::Import {
            export: ExportKind::SpotifyExtended,
        },
    }
}

/// Kullanıcı sorgusunu FTS5'in anlayacağı bir örüntüye çevirir.
///
/// FTS5 sözdizimi operatör içerir (`AND`, `"`, `*`); ham kullanıcı girdisini
/// doğrudan vermek hem hata hem güvenlik riski. Her kelime tırnaklanır ve
/// önek araması için `*` eklenir.
fn fts_pattern(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| {
            let escaped = word.replace('"', "");
            format!("\"{escaped}\"*")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl CatalogStore for SqliteLibrary {
    fn replace_catalog(
        &mut self,
        provider: &ProviderId,
        tracks: &[CatalogTrack],
    ) -> Result<CatalogWriteSummary> {
        let now = jiff::Timestamp::now().as_millisecond();
        let tx = self
            .conn
            .transaction()
            .map_err(db_err(Stage::LibraryWrite))?;
        let mut summary = CatalogWriteSummary::default();

        // Bu turda görülen referanslar; kalanlar silinecek.
        let mut seen: std::collections::HashSet<&str> =
            std::collections::HashSet::with_capacity(tracks.len());

        for entry in tracks {
            seen.insert(entry.id.id.as_str());
            let key = track_key(&entry.track.artist, &entry.track.title);
            let duration_ms = entry
                .track
                .duration_ms
                .and_then(|ms| i64::try_from(ms).ok());

            let existing: Option<(i64, Option<i64>)> = tx
                .query_row(
                    "SELECT id, mtime_ms FROM provider_tracks
                     WHERE provider = ?1 AND provider_ref = ?2",
                    rusqlite::params![provider.as_str(), entry.id.id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional_row()?;

            match existing {
                // Dosya değişmemiş: dokunma. Taramanın pahalı kısmını atlar.
                Some((_, mtime)) if mtime.is_some() && mtime == entry.mtime_ms => {
                    summary.unchanged += 1;
                }
                Some((id, _)) => {
                    tx.execute(
                        "UPDATE provider_tracks SET
                             norm_key = ?2, artist = ?3, title = ?4, album = ?5,
                             duration_ms = ?6, isrc = ?7, from_tags = ?8,
                             mtime_ms = ?9, scanned_at = ?10
                         WHERE id = ?1",
                        rusqlite::params![
                            id,
                            key,
                            entry.track.artist,
                            entry.track.title,
                            entry.track.album,
                            duration_ms,
                            entry.track.isrc.as_ref().map(Isrc::as_str),
                            i64::from(entry.from_tags),
                            entry.mtime_ms,
                            now,
                        ],
                    )
                    .map_err(db_err(Stage::LibraryWrite))?;
                    summary.updated += 1;
                }
                None => {
                    tx.execute(
                        "INSERT INTO provider_tracks
                             (provider, provider_ref, norm_key, artist, title, album,
                              duration_ms, isrc, from_tags, mtime_ms, scanned_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                        rusqlite::params![
                            provider.as_str(),
                            entry.id.id.as_str(),
                            key,
                            entry.track.artist,
                            entry.track.title,
                            entry.track.album,
                            duration_ms,
                            entry.track.isrc.as_ref().map(Isrc::as_str),
                            i64::from(entry.from_tags),
                            entry.mtime_ms,
                            now,
                        ],
                    )
                    .map_err(db_err(Stage::LibraryWrite))?;
                    summary.inserted += 1;
                }
            }
        }

        // Kaynakta artık olmayanları düş: katalog kaynağın aynasıdır.
        // Silinen dosyanın **geçmişi** silinmez; o `listens` tablosunda durur.
        let stale: Vec<(i64, String)> = {
            let mut stmt = tx
                .prepare("SELECT id, provider_ref FROM provider_tracks WHERE provider = ?1")
                .map_err(db_err(Stage::LibraryQuery))?;
            let rows = stmt
                .query_map([provider.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(db_err(Stage::LibraryQuery))?;
            let mut out = Vec::new();
            for row in rows {
                let (id, reference): (i64, String) = row.map_err(db_err(Stage::LibraryQuery))?;
                if !seen.contains(reference.as_str()) {
                    out.push((id, reference));
                }
            }
            out
        };
        for (id, reference) in stale {
            tx.execute("DELETE FROM provider_tracks WHERE id = ?1", [id])
                .map_err(db_err(Stage::LibraryWrite))?;
            tracing::debug!(referans = %reference, "katalogdan düştü");
            summary.removed += 1;
        }

        tx.commit().map_err(db_err(Stage::LibraryWrite))?;
        Ok(summary)
    }

    fn search_catalog(&self, query: &str, limit: usize) -> Result<Vec<CatalogTrack>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.provider, p.provider_ref, p.artist, p.title, p.album,
                        p.duration_ms, p.isrc, p.from_tags, p.mtime_ms
                 FROM provider_tracks_fts f
                 JOIN provider_tracks p ON p.id = f.rowid
                 WHERE provider_tracks_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(db_err(Stage::LibraryQuery))?;

        let rows = stmt
            .query_map(
                rusqlite::params![fts_pattern(query), i64::try_from(limit).unwrap_or(i64::MAX)],
                catalog_row,
            )
            .map_err(db_err(Stage::LibraryQuery))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err(Stage::LibraryQuery))?);
        }
        Ok(out)
    }

    fn catalog_len(&self, provider: &ProviderId) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM provider_tracks WHERE provider = ?1",
                [provider.as_str()],
                |row| row.get(0),
            )
            .map_err(db_err(Stage::LibraryQuery))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    fn catalog_get(&self, id: &ProviderTrackId) -> Result<Option<CatalogTrack>> {
        self.conn
            .query_row(
                "SELECT provider, provider_ref, artist, title, album,
                        duration_ms, isrc, from_tags, mtime_ms
                 FROM provider_tracks
                 WHERE provider = ?1 AND provider_ref = ?2",
                rusqlite::params![id.provider.as_str(), id.id.as_str()],
                catalog_row,
            )
            .optional_row()
    }

    fn catalog_stamps(
        &self,
        provider: &ProviderId,
    ) -> Result<std::collections::HashMap<String, i64>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT provider_ref, mtime_ms FROM provider_tracks
                 WHERE provider = ?1 AND mtime_ms IS NOT NULL",
            )
            .map_err(db_err(Stage::LibraryQuery))?;
        let rows = stmt
            .query_map([provider.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(db_err(Stage::LibraryQuery))?;

        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (reference, mtime) = row.map_err(db_err(Stage::LibraryQuery))?;
            out.insert(reference, mtime);
        }
        Ok(out)
    }
}

/// Katalog satırını okur. Sütun sırası sorgularla birebir aynı olmalı.
fn catalog_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CatalogTrack> {
    let provider: String = row.get(0)?;
    let reference: String = row.get(1)?;
    let artist: String = row.get(2)?;
    let title: String = row.get(3)?;
    let album: Option<String> = row.get(4)?;
    let duration_ms: Option<i64> = row.get(5)?;
    let isrc: Option<String> = row.get(6)?;
    let from_tags: i64 = row.get(7)?;
    let mtime_ms: Option<i64> = row.get(8)?;

    Ok(CatalogTrack {
        id: ProviderTrackId::new(ProviderId::new(provider), reference),
        track: TrackRef::new(artist, title)
            .with_album(album)
            .with_duration_ms(duration_ms.and_then(|ms| u64::try_from(ms).ok()))
            .with_isrc(isrc.as_deref().and_then(Isrc::parse)),
        from_tags: from_tags != 0,
        mtime_ms,
    })
}

/// `ResolveMethod`'u metinden geri okur — rapor katmanı için.
#[must_use]
pub fn parse_resolve_method(raw: &str) -> Option<ResolveMethod> {
    match raw {
        "isrc" => Some(ResolveMethod::Isrc),
        "mbid" => Some(ResolveMethod::Mbid),
        "fuzzy" => Some(ResolveMethod::Fuzzy),
        "fingerprint" => Some(ResolveMethod::Fingerprint),
        "local_key" => Some(ResolveMethod::LocalKey),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ExportKind;

    fn listen(artist: &str, title: &str, ts: &str, ms: u64) -> Listen {
        Listen {
            track: TrackRef::new(artist, title).with_album(Some("Pablo Honey".to_owned())),
            played_at: ts.parse().expect("test zaman damgası"),
            ms_played: ms,
            source: ListenSource::Import {
                export: ExportKind::SpotifyExtended,
            },
            canonical_id: None,
        }
    }

    #[test]
    fn schema_applies_and_fts5_is_available() {
        let library = SqliteLibrary::open_in_memory().expect("bellek içi kütüphane açılmalı");
        let version: i64 = library
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("sürüm okunmalı");
        assert_eq!(usize::try_from(version).unwrap(), schema::MIGRATIONS.len());
    }

    #[test]
    fn reimport_does_not_double_count() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let batch = vec![
            listen("Radiohead", "Creep", "2023-01-01T00:00:00Z", 200_000),
            listen("Radiohead", "Karma Police", "2023-01-01T00:05:00Z", 260_000),
        ];

        let first = library.insert_listens(&batch).unwrap();
        assert_eq!(first.inserted, 2);
        assert_eq!(first.new_tracks, 2);
        assert_eq!(first.duplicates, 0);

        let second = library.insert_listens(&batch).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.duplicates, 2);
        assert_eq!(second.new_tracks, 0);
        assert_eq!(library.all_listens().unwrap().len(), 2);
    }

    #[test]
    fn listens_round_trip_through_storage() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let original = listen("Portishead", "Roads", "2024-03-05T10:11:12Z", 303_000);
        library
            .insert_listens(std::slice::from_ref(&original))
            .unwrap();

        let loaded = library.all_listens().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].track.artist, "Portishead");
        assert_eq!(loaded[0].track.album.as_deref(), Some("Pablo Honey"));
        assert_eq!(loaded[0].played_at, original.played_at);
        assert_eq!(loaded[0].ms_played, 303_000);
    }

    #[test]
    fn full_text_search_finds_prefixes_and_counts_plays() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        library
            .insert_listens(&[
                listen("Radiohead", "Creep", "2023-01-01T00:00:00Z", 200_000),
                listen("Radiohead", "Creep", "2023-01-02T00:00:00Z", 200_000),
                listen("Portishead", "Roads", "2023-01-03T00:00:00Z", 300_000),
            ])
            .unwrap();

        let rule = PlayRule::default();
        let found = library.search("radio", 10, rule).unwrap();
        assert_eq!(
            found.hits.len(),
            1,
            "önek araması Radiohead'i bulmalı: {found:?}"
        );
        assert_eq!(found.hits[0].play_count, 2);
        assert_eq!(found.hits[0].ms_played, 400_000);
        assert_eq!(found.listen_events, 2);

        assert_eq!(library.search("creep", 10, rule).unwrap().hits.len(), 1);
        assert!(
            library
                .search("bulunmayanparça", 10, rule)
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn search_counts_the_same_plays_as_stats() {
        // D-008: `search` ham olayları, `stats` eşikli çalmaları sayıyordu;
        // aynı veri iki farklı "çalma" sayısı veriyordu. Artık tek kural.
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let batch = [
            listen("Radiohead", "Creep", "2023-01-01T00:00:00Z", 200_000),
            listen("Radiohead", "Creep", "2023-01-02T00:00:00Z", 200_000),
            // Eşiğin altında: iki yüzeyde de "çalma" sayılmamalı.
            listen("Radiohead", "Creep", "2023-01-03T00:00:00Z", 4_000),
        ];
        library.insert_listens(&batch).unwrap();

        let rule = PlayRule::default();
        let found = library.search("creep", 10, rule).unwrap();
        let stats = crate::stats::compute(
            &library.all_listens().unwrap(),
            crate::stats::StatsQuery::default(),
        );

        assert_eq!(found.hits[0].play_count, stats.plays);
        assert_eq!(found.hits[0].ms_played, stats.total_ms_played);
        assert_eq!(
            found.listen_events, stats.listens_in_scope,
            "ham olay sayısı da örtüşmeli"
        );
        assert_eq!(found.listen_events - found.hits[0].play_count, 1);
    }

    #[test]
    fn sql_play_rule_agrees_with_rust() {
        // `play_predicate_sql` ile `PlayRule::counts` ayrışırsa D-008 geri döner.
        // Kenar durumlar tek tek, gerçek SQLite üzerinden karşılaştırılıyor.
        let rule = PlayRule::default();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let sql = format!(
            "SELECT {} FROM (SELECT ?1 AS ms_played) l, (SELECT ?2 AS duration_ms) t",
            play_predicate_sql(rule)
        );

        let cases: &[(u64, Option<u64>)] = &[
            (0, None),
            (0, Some(0)),
            (1, Some(0)),
            (29_999, None),
            (30_000, None),
            (30_001, None),
            (25_000, Some(40_000)),
            (25_000, Some(50_000)),
            (25_000, Some(50_001)),
            (25_000, Some(240_000)),
            (200_000, Some(238_000)),
        ];

        for &(ms_played, duration_ms) in cases {
            let sql_says: bool = conn
                .query_row(
                    &sql,
                    rusqlite::params![
                        i64::try_from(ms_played).unwrap(),
                        duration_ms.map(|d| i64::try_from(d).unwrap()),
                    ],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                sql_says,
                rule.counts(ms_played, duration_ms),
                "ms_played={ms_played}, duration_ms={duration_ms:?}"
            );
        }
    }

    #[test]
    fn fts_special_characters_do_not_break_the_query() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        library
            .insert_listens(&[listen(
                "AC/DC",
                "Back in Black",
                "2023-01-01T00:00:00Z",
                255_000,
            )])
            .unwrap();

        // Ham girdideki tırnak ve FTS5 operatörleri sözdizimi hatası vermemeli.
        for raw in ["\"AC OR DC", "back*", "NOT black", "(", "a\"b"] {
            library
                .search(raw, 10, PlayRule::default())
                .unwrap_or_else(|err| panic!("{raw:?} sorgusu patladı: {}", err.chain_text()));
        }

        let rule = PlayRule::default();
        assert_eq!(library.search("AC/DC", 10, rule).unwrap().hits.len(), 1);
        assert_eq!(
            library.search("back black", 10, rule).unwrap().hits.len(),
            1
        );
    }

    #[test]
    fn empty_search_is_an_error_not_an_empty_list() {
        let library = SqliteLibrary::open_in_memory().unwrap();
        let err = library.search("   ", 10, PlayRule::default()).unwrap_err();
        assert_eq!(err.stage(), Stage::LibraryQuery);
    }

    #[test]
    fn resolution_is_persisted_and_read_back() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        library
            .insert_listens(&[listen(
                "Radiohead",
                "Creep",
                "2023-01-01T00:00:00Z",
                200_000,
            )])
            .unwrap();

        let resolution = Resolution {
            canonical_id: CanonicalId::from_local_key("radiohead\u{1}creep"),
            method: ResolveMethod::LocalKey,
            confidence: 0.2,
            matched: None,
        };
        library
            .set_resolution(&track_key("Radiohead", "Creep"), &resolution)
            .unwrap();

        let loaded = library.all_listens().unwrap();
        assert_eq!(loaded[0].canonical_id, Some(resolution.canonical_id));
    }

    // — Kalıcı katalog (Faz 1.2) —

    fn local_id(reference: &str) -> ProviderTrackId {
        ProviderTrackId::new(ProviderId::new("local"), reference)
    }

    fn catalog_entry(reference: &str, artist: &str, title: &str, mtime: i64) -> CatalogTrack {
        CatalogTrack {
            id: local_id(reference),
            track: TrackRef::new(artist, title).with_album(Some("Albüm".to_owned())),
            from_tags: true,
            mtime_ms: Some(mtime),
        }
    }

    #[test]
    fn catalog_survives_reopening_the_database() {
        // Asıl amaç buydu: indeks bellekte değil diskte.
        let dir = std::env::temp_dir().join(format!(
            "tune-catalog-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("library.db");
        let provider = ProviderId::new("local");

        {
            let mut library = SqliteLibrary::open(&db).unwrap();
            library
                .replace_catalog(
                    &provider,
                    &[catalog_entry("/m/a.flac", "Radiohead", "Creep", 100)],
                )
                .unwrap();
        }

        // Yeni süreç gibi: veritabanını baştan aç.
        let library = SqliteLibrary::open(&db).unwrap();
        assert_eq!(library.catalog_len(&provider).unwrap(), 1);
        let hits = library.search_catalog("radiohead", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].track.title, "Creep");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replacing_the_catalog_drops_rows_that_vanished() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let provider = ProviderId::new("local");

        let write = library
            .replace_catalog(
                &provider,
                &[
                    catalog_entry("/m/a.flac", "Radiohead", "Creep", 100),
                    catalog_entry("/m/b.flac", "Portishead", "Roads", 100),
                ],
            )
            .unwrap();
        assert_eq!(write.inserted, 2);

        // İkinci dosya silinmiş: katalog kaynağın aynası olmalı.
        let write = library
            .replace_catalog(
                &provider,
                &[catalog_entry("/m/a.flac", "Radiohead", "Creep", 100)],
            )
            .unwrap();
        assert_eq!(write.removed, 1);
        assert_eq!(library.catalog_len(&provider).unwrap(), 1);
        assert!(library.search_catalog("portishead", 10).unwrap().is_empty());
    }

    #[test]
    fn dropping_a_file_from_the_catalog_never_touches_its_history() {
        // Diskten sildiğin dosyanın **geçmişi** silinmez. `tracks`/`listens`
        // ile `provider_tracks` bilerek ayrı tablolar.
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let provider = ProviderId::new("local");

        library
            .insert_listens(&[listen(
                "Radiohead",
                "Creep",
                "2023-01-01T00:00:00Z",
                200_000,
            )])
            .unwrap();
        library
            .replace_catalog(
                &provider,
                &[catalog_entry("/m/a.flac", "Radiohead", "Creep", 100)],
            )
            .unwrap();

        // Dosya diskten silindi → katalog boşaldı.
        library.replace_catalog(&provider, &[]).unwrap();
        assert_eq!(library.catalog_len(&provider).unwrap(), 0);

        // Geçmiş yerinde.
        assert_eq!(library.all_listens().unwrap().len(), 1);
        let hits = library.search("creep", 10, PlayRule::default()).unwrap();
        assert_eq!(hits.hits.len(), 1, "dinleme geçmişi aramada durmalı");
    }

    #[test]
    fn an_unchanged_stamp_is_reported_and_the_row_is_left_alone() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let provider = ProviderId::new("local");
        let row = catalog_entry("/m/a.flac", "Radiohead", "Creep", 100);

        library
            .replace_catalog(&provider, std::slice::from_ref(&row))
            .unwrap();
        let write = library.replace_catalog(&provider, &[row]).unwrap();

        assert_eq!(write.unchanged, 1, "aynı damga yeniden yazılmamalı");
        assert_eq!(write.updated, 0);
        assert_eq!(write.inserted, 0);
    }

    #[test]
    fn a_changed_stamp_updates_the_metadata() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let provider = ProviderId::new("local");

        library
            .replace_catalog(
                &provider,
                &[catalog_entry("/m/a.flac", "Eski Ad", "Eski Başlık", 100)],
            )
            .unwrap();
        // Dosya düzenlendi: damga değişti, etiketler de.
        let write = library
            .replace_catalog(
                &provider,
                &[catalog_entry("/m/a.flac", "Yeni Ad", "Yeni Başlık", 200)],
            )
            .unwrap();

        assert_eq!(write.updated, 1);
        let hits = library.search_catalog("yeni", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].track.artist, "Yeni Ad");
        // Eski üstveri FTS'te kalmamalı.
        assert!(library.search_catalog("eski", 10).unwrap().is_empty());
    }

    #[test]
    fn stamps_are_returned_for_incremental_scanning() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let provider = ProviderId::new("local");
        library
            .replace_catalog(
                &provider,
                &[
                    catalog_entry("/m/a.flac", "A", "1", 111),
                    catalog_entry("/m/b.flac", "B", "2", 222),
                ],
            )
            .unwrap();

        let stamps = library.catalog_stamps(&provider).unwrap();
        assert_eq!(stamps.get("/m/a.flac"), Some(&111));
        assert_eq!(stamps.get("/m/b.flac"), Some(&222));
    }

    #[test]
    fn catalogs_of_different_providers_do_not_erase_each_other() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let local = ProviderId::new("local");
        let remote = ProviderId::new("subsonic");

        library
            .replace_catalog(&local, &[catalog_entry("/m/a.flac", "A", "1", 100)])
            .unwrap();
        library
            .replace_catalog(
                &remote,
                &[CatalogTrack {
                    id: ProviderTrackId::new(remote.clone(), "track-9"),
                    track: TrackRef::new("B", "2"),
                    from_tags: true,
                    mtime_ms: None,
                }],
            )
            .unwrap();

        // Uzak sağlayıcıyı yazmak yereli düşürmemeli.
        assert_eq!(library.catalog_len(&local).unwrap(), 1);
        assert_eq!(library.catalog_len(&remote).unwrap(), 1);
    }

    #[test]
    fn catalog_get_finds_a_row_by_provider_id() {
        let mut library = SqliteLibrary::open_in_memory().unwrap();
        let provider = ProviderId::new("local");
        library
            .replace_catalog(
                &provider,
                &[catalog_entry("/m/a.flac", "Radiohead", "Creep", 100)],
            )
            .unwrap();

        let found = library.catalog_get(&local_id("/m/a.flac")).unwrap();
        assert_eq!(found.map(|row| row.track.title), Some("Creep".to_owned()));
        assert_eq!(library.catalog_get(&local_id("/m/yok.flac")).unwrap(), None);
    }

    #[test]
    fn an_empty_catalog_search_is_empty_not_an_error() {
        let library = SqliteLibrary::open_in_memory().unwrap();
        assert!(library.search_catalog("", 10).unwrap().is_empty());
        assert!(library.search_catalog("  ", 10).unwrap().is_empty());
    }

    #[test]
    fn migrating_a_v1_database_keeps_its_listens() {
        // v2 göçü var olan kurulumları bozmamalı: kullanıcının import ettiği
        // geçmiş, şema yükseldiğinde yerinde kalmalı.
        let dir = std::env::temp_dir().join(format!(
            "tune-migrate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("library.db");

        // v1 şemasını elle kur ve bir dinleme yaz.
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(schema::MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO tracks (norm_key, artist, title) VALUES ('k', 'Radiohead', 'Creep')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO listens (track_id, played_at, ms_played, source_kind)
                 VALUES (1, 1700000000000, 200000, 'import')",
                [],
            )
            .unwrap();
        }

        // Açmak göçü uygular.
        let mut library = SqliteLibrary::open(&db).unwrap();
        let version: i64 = library
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2, "şema v2'ye yükselmeli");

        // Eski geçmiş yerinde.
        assert_eq!(library.all_listens().unwrap().len(), 1);
        // Yeni katalog kullanılabilir.
        let provider = ProviderId::new("local");
        library
            .replace_catalog(
                &provider,
                &[catalog_entry("/m/a.flac", "Portishead", "Roads", 1)],
            )
            .unwrap();
        assert_eq!(library.catalog_len(&provider).unwrap(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
