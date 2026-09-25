//! The library schema and its migrations.
//!
//! Migrations are ordered and irreversible; each one bumps `schema_version` by
//! one. When adding a new migration, append it to the array — **do not
//! change** an existing one.

/// The migrations, applied in order. Index in the array + 1 = schema version.
pub(crate) const MIGRATIONS: &[&str] = &[
    // v1 — tracks, listens and full-text search.
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
    // v2 — the provider catalog (Phase 1.2).
    //
    // The local file index used to live in memory for the life of the process;
    // every `play` call rescanned the disk from scratch. This table makes the
    // index persistent.
    //
    // It is kept **separate** from `listens`/`tracks`, because they are different
    // things: `tracks` is "the tracks you listened to" (derived from raw events,
    // never deleted), while `provider_tracks` is "what you can play right now" —
    // when a file is deleted, its row goes too. Merging the two would mean
    // deleting the history of a file you deleted from disk as well.
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
        -- Read from the tags (1), or derived from the file name (0).
        from_tags    INTEGER NOT NULL DEFAULT 0,
        -- The file's last modification time (ms). If unchanged, we do not read it again.
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
