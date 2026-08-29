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
    // v2 — sağlayıcı kataloğu (Faz 1.2).
    //
    // Yerel dosya indeksi süreç ömrü boyunca bellekte duruyordu; her `play`
    // çağrısı diski baştan tarıyordu. Bu tablo indeksi kalıcı kılar.
    //
    // `listens`/`tracks`'tan **ayrı** tutuluyor, çünkü farklı şeyler:
    // `tracks` "dinlediğin parçalar" (ham olaylardan türer, asla silinmez),
    // `provider_tracks` ise "şu an çalabildiklerin" — dosya silinince satır
    // da gider. İkisini birleştirmek, diskten sildiğin bir dosyanın geçmişini
    // de silmek olurdu.
    r"
    CREATE TABLE provider_tracks (
        id           INTEGER PRIMARY KEY,
        provider     TEXT    NOT NULL,
        provider_ref TEXT    NOT NULL,
        norm_key     TEXT    NOT NULL,
        artist       TEXT    NOT NULL,
        title        TEXT    NOT NULL,
        album        TEXT,
        duration_ms  INTEGER,
        isrc         TEXT,
        -- Etiketlerden mi okundu (1), dosya adından mı türetildi (0).
        from_tags    INTEGER NOT NULL DEFAULT 0,
        -- Dosyanın son değişme zamanı (ms). Değişmediyse yeniden okumayız.
        mtime_ms     INTEGER,
        scanned_at   INTEGER NOT NULL,
        UNIQUE(provider, provider_ref)
    );
    CREATE INDEX provider_tracks_norm_key ON provider_tracks(norm_key);
    CREATE INDEX provider_tracks_provider ON provider_tracks(provider);

    CREATE VIRTUAL TABLE provider_tracks_fts USING fts5(
        artist, title, album,
        content='provider_tracks', content_rowid='id'
    );

    CREATE TRIGGER provider_tracks_fts_insert AFTER INSERT ON provider_tracks BEGIN
        INSERT INTO provider_tracks_fts(rowid, artist, title, album)
        VALUES (new.id, new.artist, new.title, new.album);
    END;

    CREATE TRIGGER provider_tracks_fts_delete AFTER DELETE ON provider_tracks BEGIN
        INSERT INTO provider_tracks_fts(provider_tracks_fts, rowid, artist, title, album)
        VALUES ('delete', old.id, old.artist, old.title, old.album);
    END;

    CREATE TRIGGER provider_tracks_fts_update AFTER UPDATE ON provider_tracks BEGIN
        INSERT INTO provider_tracks_fts(provider_tracks_fts, rowid, artist, title, album)
        VALUES ('delete', old.id, old.artist, old.title, old.album);
        INSERT INTO provider_tracks_fts(rowid, artist, title, album)
        VALUES (new.id, new.artist, new.title, new.album);
    END;
    ",
];
