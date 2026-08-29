//! Kütüphane şeması ve göçleri.
//!
//! Göçler sıralı ve geri dönüşsüzdür; her biri `schema_version`'ı bir artırır.
//! Yeni bir göç eklerken diziye ekle, var olanı **değiştirme**.

/// Sırayla uygulanan göçler. Dizideki indeks + 1 = şema sürümü.
pub(crate) const MIGRATIONS: &[&str] = &[
    // v1 — parçalar, dinlemeler ve tam metin arama.
    r"
    CREATE TABLE tracks (
        id                INTEGER PRIMARY KEY,
        norm_key          TEXT    NOT NULL UNIQUE,
        canonical_id      TEXT,
        resolve_method    TEXT,
        resolve_confidence REAL,
        artist            TEXT    NOT NULL,
        title             TEXT    NOT NULL,
        album             TEXT,
        duration_ms       INTEGER,
        isrc              TEXT,
        provider          TEXT,
        provider_track_id TEXT
    );
    CREATE INDEX tracks_canonical_id ON tracks(canonical_id);

    CREATE TABLE listens (
        id          INTEGER PRIMARY KEY,
        track_id    INTEGER NOT NULL REFERENCES tracks(id),
        played_at   INTEGER NOT NULL,
        ms_played   INTEGER NOT NULL,
        source_kind TEXT    NOT NULL,
        source_ref  TEXT,
        UNIQUE(track_id, played_at, ms_played)
    );
    CREATE INDEX listens_played_at ON listens(played_at);

    CREATE VIRTUAL TABLE tracks_fts USING fts5(
        artist, title, album,
        content='tracks', content_rowid='id'
    );

    CREATE TRIGGER tracks_fts_insert AFTER INSERT ON tracks BEGIN
        INSERT INTO tracks_fts(rowid, artist, title, album)
        VALUES (new.id, new.artist, new.title, new.album);
    END;

    CREATE TRIGGER tracks_fts_delete AFTER DELETE ON tracks BEGIN
        INSERT INTO tracks_fts(tracks_fts, rowid, artist, title, album)
        VALUES ('delete', old.id, old.artist, old.title, old.album);
    END;

    CREATE TRIGGER tracks_fts_update AFTER UPDATE ON tracks BEGIN
        INSERT INTO tracks_fts(tracks_fts, rowid, artist, title, album)
        VALUES ('delete', old.id, old.artist, old.title, old.album);
        INSERT INTO tracks_fts(rowid, artist, title, album)
        VALUES (new.id, new.artist, new.title, new.album);
    END;
    ",
];
