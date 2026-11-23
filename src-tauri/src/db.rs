use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::models::{LibraryPayload, LibrarySource, MediaItem, MetadataPatch, MusicCollectionItem, SeasonItem, SeriesItem};
use crate::scanner::ScannedMedia;

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS library_sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE COLLATE NOCASE,
            source_type TEXT NOT NULL DEFAULT 'auto',
            is_primary INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1,
            added_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_library_sources_enabled ON library_sources(enabled, is_primary DESC);

        CREATE TABLE IF NOT EXISTS media (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            auto_title TEXT NOT NULL,
            title TEXT NOT NULL,
            title_locked INTEGER NOT NULL DEFAULT 0,
            description TEXT NOT NULL DEFAULT '',
            file_path TEXT NOT NULL UNIQUE,
            series_key TEXT,
            series_title TEXT,
            season_number INTEGER,
            episode_number INTEGER,
            artist TEXT,
            album TEXT,
            music_collection_key TEXT,
            thumbnail_locked INTEGER NOT NULL DEFAULT 0,
            duration REAL NOT NULL DEFAULT 0,
            year INTEGER,
            poster_path TEXT,
            backdrop_path TEXT,
            thumbnail_path TEXT,
            added_at INTEGER NOT NULL,
            modified_at INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_media_kind ON media(kind);
        CREATE INDEX IF NOT EXISTS idx_media_series ON media(series_key, season_number, episode_number);
        CREATE INDEX IF NOT EXISTS idx_media_added ON media(added_at DESC);

        CREATE TABLE IF NOT EXISTS series (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            series_key TEXT NOT NULL UNIQUE,
            auto_title TEXT NOT NULL,
            title TEXT NOT NULL,
            title_locked INTEGER NOT NULL DEFAULT 0,
            description TEXT NOT NULL DEFAULT '',
            poster_path TEXT,
            backdrop_path TEXT
        );

        CREATE TABLE IF NOT EXISTS music_collections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            collection_key TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            poster_path TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_music_collections_title
            ON music_collections(title COLLATE NOCASE);

        CREATE TABLE IF NOT EXISTS progress (
            media_id INTEGER PRIMARY KEY,
            position REAL NOT NULL DEFAULT 0,
            duration REAL NOT NULL DEFAULT 0,
            last_watched_at INTEGER NOT NULL,
            completed INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(media_id) REFERENCES media(id) ON DELETE CASCADE
        );
        "#,
    )
    .map_err(|e| e.to_string())?;

    // Existing HED Space databases from pre-0.2 builds are upgraded in place.
    ensure_column(conn, "media", "year", "INTEGER")?;
    ensure_column(conn, "media", "music_collection_key", "TEXT")?;
    ensure_column(conn, "media", "thumbnail_locked", "INTEGER NOT NULL DEFAULT 0")?;

    // Upgrade legacy single-root installs once, but do not keep a special/locked
    // "primary" source. From 0.3.6 every folder is just a typed library source.
    let legacy_root: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'media_root'", [], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(root) = legacy_root {
        conn.execute(
            r#"INSERT INTO library_sources(path, source_type, is_primary, enabled, added_at)
               VALUES(?1, 'auto', 0, 1, strftime('%s','now'))
               ON CONFLICT(path) DO NOTHING"#,
            [&root],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute("UPDATE library_sources SET is_primary = 0", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"), [])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}
pub fn list_library_sources(conn: &Connection) -> Result<Vec<LibrarySource>, String> {
    let mut stmt = conn
        .prepare("SELECT id, path, source_type, is_primary, enabled FROM library_sources ORDER BY id ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(LibrarySource {
                id: row.get(0)?,
                path: row.get(1)?,
                source_type: row.get(2)?,
                is_primary: row.get::<_, i64>(3)? != 0,
                enabled: row.get::<_, i64>(4)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

pub fn add_library_source(conn: &Connection, path: &str, source_type: &str) -> Result<(), String> {
    let allowed = matches!(source_type, "auto" | "standard" | "movies" | "series" | "anime" | "childhood" | "songs" | "atmospheres");
    if !allowed {
        return Err("Unsupported library source type".into());
    }
    conn.execute(
        r#"INSERT INTO library_sources(path, source_type, is_primary, enabled, added_at)
           VALUES(?1, ?2, 0, 1, strftime('%s','now'))
           ON CONFLICT(path) DO UPDATE SET source_type = excluded.source_type, enabled = 1"#,
        params![path, source_type],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_library_source_type(conn: &Connection, id: i64, source_type: &str) -> Result<(), String> {
    let allowed = matches!(source_type, "auto" | "standard" | "movies" | "series" | "anime" | "childhood" | "songs" | "atmospheres");
    if !allowed {
        return Err("Unsupported library source type".into());
    }
    conn.execute(
        "UPDATE library_sources SET source_type = ?1, enabled = 1 WHERE id = ?2",
        params![source_type, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_library_source(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM library_sources WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn begin_scan(conn: &Connection, scan_started_at: i64) -> Result<(), String> {
    // Set a marker lower than the upcoming scan timestamp. Records touched by this scan are updated.
    conn.execute(
        "UPDATE media SET last_seen_at = MIN(last_seen_at, ?1 - 1)",
        [scan_started_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn upsert_media(conn: &Connection, item: &ScannedMedia, scan_at: i64) -> Result<i64, String> {
    conn.execute(
        r#"
        INSERT INTO media(
            kind, auto_title, title, file_path, series_key, series_title,
            season_number, episode_number, artist, album, music_collection_key,
            duration, year, poster_path, backdrop_path, thumbnail_path,
            added_at, modified_at, last_seen_at
        ) VALUES (
            ?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
        )
        ON CONFLICT(file_path) DO UPDATE SET
            kind = excluded.kind,
            auto_title = excluded.auto_title,
            title = CASE WHEN media.title_locked = 0 THEN excluded.auto_title ELSE media.title END,
            series_key = excluded.series_key,
            series_title = excluded.series_title,
            season_number = excluded.season_number,
            episode_number = excluded.episode_number,
            artist = CASE WHEN COALESCE(media.artist, '') = '' THEN excluded.artist ELSE media.artist END,
            album = CASE WHEN COALESCE(media.album, '') = '' THEN excluded.album ELSE media.album END,
            music_collection_key = excluded.music_collection_key,
            duration = CASE WHEN excluded.duration > 0 THEN excluded.duration ELSE media.duration END,
            year = COALESCE(excluded.year, media.year),
            poster_path = COALESCE(media.poster_path, excluded.poster_path),
            backdrop_path = COALESCE(media.backdrop_path, excluded.backdrop_path),
            thumbnail_path = CASE
                WHEN media.thumbnail_locked = 1 THEN media.thumbnail_path
                ELSE COALESCE(excluded.thumbnail_path, media.thumbnail_path)
            END,
            modified_at = excluded.modified_at,
            last_seen_at = excluded.last_seen_at
        "#,
        params![
            &item.kind,
            &item.title,
            &item.file_path,
            item.series_key.as_deref(),
            item.series_title.as_deref(),
            item.season_number,
            item.episode_number,
            item.artist.as_deref(),
            item.album.as_deref(),
            item.music_collection_key.as_deref(),
            item.duration,
            item.year,
            item.poster_path.as_deref(),
            item.backdrop_path.as_deref(),
            item.thumbnail_path.as_deref(),
            item.added_at,
            item.modified_at,
            scan_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    let id = conn
        .query_row(
            "SELECT id FROM media WHERE file_path = ?1",
            [&item.file_path],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn ensure_series(
    conn: &Connection,
    series_key: &str,
    series_title: &str,
) -> Result<i64, String> {
    conn.execute(
        r#"
        INSERT INTO series(series_key, auto_title, title)
        VALUES(?1, ?2, ?2)
        ON CONFLICT(series_key) DO UPDATE SET
            auto_title = excluded.auto_title,
            title = CASE WHEN series.title_locked = 0 THEN excluded.auto_title ELSE series.title END
        "#,
        params![series_key, series_title],
    )
    .map_err(|e| e.to_string())?;
    let (id, display_title): (i64, String) = conn
        .query_row(
            "SELECT id, title FROM series WHERE series_key = ?1",
            [series_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    // Keep episode-facing series_title aligned with a user-locked series display name after rescans.
    conn.execute(
        "UPDATE media SET series_title = ?1 WHERE series_key = ?2",
        params![display_title, series_key],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn finish_scan(conn: &Connection, scan_at: i64) -> Result<(), String> {
    conn.execute("DELETE FROM media WHERE last_seen_at < ?1", [scan_at])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM series WHERE series_key NOT IN (SELECT DISTINCT series_key FROM media WHERE series_key IS NOT NULL)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn media_from_row(row: &Row<'_>) -> rusqlite::Result<MediaItem> {
    Ok(MediaItem {
        id: row.get("id")?,
        kind: row.get("kind")?,
        title: row.get("title")?,
        description: row.get("description")?,
        file_path: row.get("file_path")?,
        series_key: row.get("series_key")?,
        series_title: row.get("series_title")?,
        season_number: row.get("season_number")?,
        episode_number: row.get("episode_number")?,
        artist: row.get("artist")?,
        album: row.get("album")?,
        music_collection_key: row.get("music_collection_key")?,
        thumbnail_locked: row.get::<_, i64>("thumbnail_locked")? != 0,
        duration: row.get("duration")?,
        year: row.get("year")?,
        poster_path: row.get("poster_path")?,
        backdrop_path: row.get("backdrop_path")?,
        thumbnail_path: row.get("thumbnail_path")?,
        added_at: row.get("added_at")?,
        modified_at: row.get("modified_at")?,
        progress_position: row.get("progress_position")?,
        progress_duration: row.get("progress_duration")?,
        last_watched_at: row.get("last_watched_at")?,
    })
}

const MEDIA_SELECT: &str = r#"
    SELECT
        m.*,
        p.position AS progress_position,
        p.duration AS progress_duration,
        p.last_watched_at AS last_watched_at
    FROM media m
    LEFT JOIN progress p ON p.media_id = m.id
"#;

pub fn list_media_by_kind(conn: &Connection, kind: &str) -> Result<Vec<MediaItem>, String> {
    let sql = format!("{} WHERE m.kind = ?1 ORDER BY m.title COLLATE NOCASE", MEDIA_SELECT);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([kind], media_from_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub fn list_recently_added(conn: &Connection, limit: usize) -> Result<Vec<MediaItem>, String> {
    let sql = format!(
        "{} WHERE m.kind IN ('movie','episode') ORDER BY m.added_at DESC LIMIT ?1",
        MEDIA_SELECT
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit as i64], media_from_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub fn list_continue_watching(conn: &Connection, limit: usize) -> Result<Vec<MediaItem>, String> {
    let sql = format!(
        "{} WHERE p.position > 30 AND p.completed = 0 AND m.kind IN ('movie','episode') ORDER BY p.last_watched_at DESC LIMIT ?1",
        MEDIA_SELECT
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit as i64], media_from_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub fn list_series(conn: &Connection) -> Result<Vec<SeriesItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, series_key, title, description, poster_path, backdrop_path FROM series ORDER BY title COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let series_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut output = Vec::new();
    for raw in series_rows {
        let (id, series_key, title, description, poster_path, backdrop_path) =
            raw.map_err(|e| e.to_string())?;
        let sql = format!(
            "{} WHERE m.series_key = ?1 ORDER BY COALESCE(m.season_number, 0), COALESCE(m.episode_number, 0), m.title COLLATE NOCASE",
            MEDIA_SELECT
        );
        let mut ep_stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let episodes = ep_stmt
            .query_map([&series_key], media_from_row)
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;

        let mut seasons: BTreeMap<i64, Vec<MediaItem>> = BTreeMap::new();
        for episode in episodes {
            seasons
                .entry(episode.season_number.unwrap_or(1))
                .or_default()
                .push(episode);
        }
        let category = series_category_from_key(&series_key).to_string();
        output.push(SeriesItem {
            id,
            series_key,
            category,
            title,
            description,
            poster_path,
            backdrop_path,
            seasons: seasons
                .into_iter()
                .map(|(season_number, episodes)| SeasonItem {
                    season_number,
                    episodes,
                })
                .collect(),
        });
    }
    Ok(output)
}

fn series_category_from_key(series_key: &str) -> &'static str {
    if series_key.starts_with("anime::") {
        "anime"
    } else if series_key.starts_with("childhood::") {
        "childhood"
    } else {
        "series"
    }
}


pub fn ensure_music_collection(
    conn: &Connection,
    collection_key: &str,
    title: &str,
    poster_path: Option<&str>,
) -> Result<i64, String> {
    conn.execute(
        r#"
        INSERT INTO music_collections(collection_key, title, poster_path)
        VALUES(?1, ?2, ?3)
        ON CONFLICT(collection_key) DO UPDATE SET
            title = excluded.title,
            poster_path = COALESCE(music_collections.poster_path, excluded.poster_path)
        "#,
        params![collection_key, title, poster_path],
    )
    .map_err(|e| e.to_string())?;

    conn.query_row(
        "SELECT id FROM music_collections WHERE collection_key = ?1",
        [collection_key],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

pub fn list_music_collections(conn: &Connection) -> Result<Vec<MusicCollectionItem>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT mc.id, mc.collection_key, mc.title, mc.poster_path
            FROM music_collections mc
            WHERE EXISTS (
                SELECT 1 FROM media m
                WHERE m.kind = 'song' AND m.music_collection_key = mc.collection_key
            )
            ORDER BY mc.title COLLATE NOCASE
            "#,
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut output = Vec::new();
    for row in rows {
        let (id, collection_key, title, poster_path) = row.map_err(|e| e.to_string())?;
        let sql = format!(
            "{} WHERE m.kind = 'song' AND m.music_collection_key = ?1 ORDER BY m.title COLLATE NOCASE",
            MEDIA_SELECT
        );
        let mut song_stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let songs = song_stmt
            .query_map([&collection_key], media_from_row)
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;

        output.push(MusicCollectionItem {
            id,
            collection_key,
            is_soundtrack: title.eq_ignore_ascii_case("soundtracks"),
            title,
            poster_path,
            songs,
        });
    }
    Ok(output)
}

pub fn set_music_collection_artwork(
    conn: &Connection,
    id: i64,
    path: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE music_collections SET poster_path = ?1 WHERE id = ?2",
        params![path, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn library(conn: &Connection) -> Result<LibraryPayload, String> {
    Ok(LibraryPayload {
        movies: list_media_by_kind(conn, "movie")?,
        // Childhood is series-only from v0.3 onward. Legacy standalone rows are kept in
        // SQLite until a rescan touches/removes them, but are intentionally not surfaced.
        childhood: Vec::new(),
        songs: list_media_by_kind(conn, "song")?,
        music_collections: list_music_collections(conn)?,
        atmospheres: list_media_by_kind(conn, "atmosphere")?,
        series: list_series(conn)?,
        continue_watching: list_continue_watching(conn, 24)?,
        recently_added: list_recently_added(conn, 30)?,
    })
}

pub fn get_media(conn: &Connection, id: i64) -> Result<MediaItem, String> {
    let sql = format!("{} WHERE m.id = ?1", MEDIA_SELECT);
    conn.query_row(&sql, [id], media_from_row)
        .map_err(|e| e.to_string())
}

pub fn get_series(conn: &Connection, id: i64) -> Result<(String, String), String> {
    conn.query_row(
        "SELECT series_key, title FROM series WHERE id = ?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|e| e.to_string())
}

pub fn set_media_artwork(
    conn: &Connection,
    id: i64,
    art_kind: &str,
    path: &str,
) -> Result<(), String> {
    let column = match art_kind {
        "poster" => "poster_path",
        "backdrop" => "backdrop_path",
        "thumbnail" | "cover" => "thumbnail_path",
        _ => return Err("Unsupported artwork kind".into()),
    };
    let sql = if matches!(art_kind, "thumbnail" | "cover") {
        format!("UPDATE media SET {column} = ?1, thumbnail_locked = 1 WHERE id = ?2")
    } else {
        format!("UPDATE media SET {column} = ?1 WHERE id = ?2")
    };
    conn.execute(&sql, params![path, id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_series_backdrop_by_key(
    conn: &Connection,
    series_key: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT backdrop_path FROM series WHERE series_key = ?1",
        [series_key],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|value| value.flatten())
    .map_err(|e| e.to_string())
}

pub fn set_series_backdrop_if_empty(
    conn: &Connection,
    series_key: &str,
    path: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE series SET backdrop_path = ?1 WHERE series_key = ?2 AND (backdrop_path IS NULL OR TRIM(backdrop_path) = '')",
        params![path, series_key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_series_artwork(
    conn: &Connection,
    id: i64,
    art_kind: &str,
    path: &str,
) -> Result<(), String> {
    let column = match art_kind {
        "poster" => "poster_path",
        "backdrop" => "backdrop_path",
        _ => return Err("Series artwork supports poster or backdrop".into()),
    };
    let sql = format!("UPDATE series SET {column} = ?1 WHERE id = ?2");
    conn.execute(&sql, params![path, id])
        .map_err(|e| e.to_string())?;

    // HED Space series poster behavior: changing a series poster also becomes the
    // default image for every episode in that series. An individual episode can
    // still be changed afterwards with its own Image button.
    if art_kind == "poster" {
        let series_key: String = conn
            .query_row(
                "SELECT series_key FROM series WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE media SET thumbnail_path = ?1 WHERE series_key = ?2 AND kind IN ('episode', 'anime_episode', 'childhood_episode')",
            params![path, series_key],
        )
        .map_err(|e| e.to_string())?;
        // Current scanners primarily store all episodic media as kind='episode';
        // keep the broader update above future-compatible and explicitly cover it.
        conn.execute(
            "UPDATE media SET thumbnail_path = ?1 WHERE series_key = ?2 AND kind = 'episode'",
            params![path, series_key],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn update_media_metadata(
    conn: &Connection,
    id: i64,
    patch: &MetadataPatch,
) -> Result<(), String> {
    let current = get_media(conn, id)?;
    let title = patch.title.clone().unwrap_or(current.title);
    let description = patch.description.clone().unwrap_or(current.description);
    let artist = patch.artist.clone().or(current.artist);
    let album = patch.album.clone().or(current.album);
    conn.execute(
        "UPDATE media SET title = ?1, title_locked = 1, description = ?2, artist = ?3, album = ?4 WHERE id = ?5",
        params![title, description, artist, album, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_series_metadata(
    conn: &Connection,
    id: i64,
    patch: &MetadataPatch,
) -> Result<(), String> {
    let (_, current_title) = get_series(conn, id)?;
    let current_description: String = conn
        .query_row("SELECT description FROM series WHERE id = ?1", [id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let title = patch.title.clone().unwrap_or(current_title);
    let description = patch.description.clone().unwrap_or(current_description);
    conn.execute(
        "UPDATE series SET title = ?1, title_locked = 1, description = ?2 WHERE id = ?3",
        params![title, description, id],
    )
    .map_err(|e| e.to_string())?;
    let series_key: String = conn
        .query_row("SELECT series_key FROM series WHERE id = ?1", [id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE media SET series_title = ?1 WHERE series_key = ?2",
        params![title, series_key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn reset_entity_title(conn: &Connection, entity_type: &str, id: i64) -> Result<(), String> {
    match entity_type {
        "media" => {
            conn.execute(
                "UPDATE media SET title = auto_title, title_locked = 0 WHERE id = ?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
        }
        "series" => {
            let (series_key, auto_title): (String, String) = conn
                .query_row(
                    "SELECT series_key, auto_title FROM series WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE series SET title = auto_title, title_locked = 0 WHERE id = ?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE media SET series_title = ?1 WHERE series_key = ?2",
                params![auto_title, series_key],
            )
            .map_err(|e| e.to_string())?;
        }
        _ => return Err("entity_type must be media or series".into()),
    }
    Ok(())
}

pub fn save_progress(
    conn: &Connection,
    media_id: i64,
    position: f64,
    duration: f64,
    now: i64,
) -> Result<(), String> {
    let completed = duration > 0.0 && (position / duration) >= 0.90;
    let normalized_position = if completed { 0.0 } else { position.max(0.0) };
    conn.execute(
        r#"
        INSERT INTO progress(media_id, position, duration, last_watched_at, completed)
        VALUES(?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(media_id) DO UPDATE SET
            position = excluded.position,
            duration = excluded.duration,
            last_watched_at = excluded.last_watched_at,
            completed = excluded.completed
        "#,
        params![
            media_id,
            normalized_position,
            duration.max(0.0),
            now,
            if completed { 1 } else { 0 }
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn saved_position(conn: &Connection, media_id: i64) -> Result<f64, String> {
    conn.query_row(
        "SELECT position FROM progress WHERE media_id = ?1 AND completed = 0",
        [media_id],
        |row| row.get(0),
    )
    .optional()
    .map(|v| v.unwrap_or(0.0))
    .map_err(|e| e.to_string())
}

pub fn adjacent_episode_ids(
    conn: &Connection,
    media_id: i64,
) -> Result<(Option<i64>, Option<i64>), String> {
    let media = get_media(conn, media_id)?;
    let Some(series_key) = media.series_key else {
        return Ok((None, None));
    };
    let mut stmt = conn
        .prepare(
            "SELECT id FROM media WHERE series_key = ?1 ORDER BY COALESCE(season_number, 0), COALESCE(episode_number, 0), title COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([series_key], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    let Some(index) = ids.iter().position(|id| *id == media_id) else {
        return Ok((None, None));
    };
    let previous = index.checked_sub(1).and_then(|i| ids.get(i).copied());
    let next = ids.get(index + 1).copied();
    Ok((previous, next))
}
