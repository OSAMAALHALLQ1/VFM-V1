//! SQLite persistence: schema, migrations and CRUD for library data.
//!
//! The database uses `PRAGMA user_version` for migrations. All statements are
//! parameterised; no user text is ever interpolated into SQL.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{CoreError, Result};
use crate::model::{Bookmark, MediaItem, MediaKind, Playlist, Preset, WatchProgress};

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS media_items (
    id          INTEGER PRIMARY KEY,
    uri         TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL,
    title       TEXT NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    added_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS media_items_added ON media_items(added_at DESC);

CREATE TABLE IF NOT EXISTS folders (
    id         INTEGER PRIMARY KEY,
    path       TEXT NOT NULL UNIQUE,
    recursive  INTEGER NOT NULL DEFAULT 1,
    last_scan  INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS folder_media (
    folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    media_id  INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    PRIMARY KEY (folder_id, media_id)
);

CREATE TABLE IF NOT EXISTS playlists (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS playlist_items (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    media_id    INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, media_id)
);

CREATE TABLE IF NOT EXISTS queue (
    position INTEGER PRIMARY KEY,
    uri      TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS queue_meta (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS watch_progress (
    media_id   INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL,
    completed  INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS recent (
    media_id  INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
    played_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS bookmarks (
    id         INTEGER PRIMARY KEY,
    media_id   INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    position_ms INTEGER NOT NULL,
    label      TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS presets (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    payload    TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
PRAGMA user_version = 1;
";

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX)
}

pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Store { sqlite }")
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
        }
        Ok(Self { conn })
    }

    fn media_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaItem> {
        let kind: String = row.get(2)?;
        Ok(MediaItem {
            id: row.get(0)?,
            uri: row.get(1)?,
            kind: MediaKind::parse(&kind),
            title: row.get(3)?,
            duration_ms: row.get(4)?,
            size_bytes: row.get(5)?,
            added_at: row.get(6)?,
        })
    }

    /// Creates or updates a media item and returns its stable id.
    pub fn upsert_media(&self, item: &MediaItem) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO media_items (uri, kind, title, duration_ms, size_bytes, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(uri) DO UPDATE SET
                kind = excluded.kind,
                title = excluded.title,
                duration_ms = excluded.duration_ms,
                size_bytes = excluded.size_bytes",
            params![
                item.uri,
                item.kind.as_str(),
                item.title,
                item.duration_ms,
                item.size_bytes,
                if item.added_at == 0 {
                    now_ms()
                } else {
                    item.added_at
                },
            ],
        )?;
        self.media_by_uri(&item.uri)?
            .map(|m| m.id)
            .ok_or_else(|| CoreError::Invalid("upsert did not return a row".into()))
    }

    pub fn media_by_uri(&self, uri: &str) -> Result<Option<MediaItem>> {
        self.conn
            .query_row(
                "SELECT id, uri, kind, title, duration_ms, size_bytes, added_at
                 FROM media_items WHERE uri = ?1",
                [uri],
                Self::media_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn media_by_id(&self, id: i64) -> Result<Option<MediaItem>> {
        self.conn
            .query_row(
                "SELECT id, uri, kind, title, duration_ms, size_bytes, added_at
                 FROM media_items WHERE id = ?1",
                [id],
                Self::media_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_media(&self, limit: i64, offset: i64) -> Result<Vec<MediaItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uri, kind, title, duration_ms, size_bytes, added_at
             FROM media_items ORDER BY added_at DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map([limit, offset], Self::media_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Records a play: upserts the item, touches `recent` and, when the caller
    /// supplies a position, stores watch progress in the same transaction.
    pub fn record_play(
        &self,
        uri: &str,
        title: &str,
        kind: MediaKind,
        duration_ms: i64,
    ) -> Result<i64> {
        // `Connection` is not Sync, so this Store is confined to one thread;
        // unchecked_transaction avoids requiring `&mut self` on every reader.
        let tx = self.conn.unchecked_transaction()?;
        let added = now_ms();
        tx.execute(
            "INSERT INTO media_items (uri, kind, title, duration_ms, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(uri) DO UPDATE SET title = excluded.title, duration_ms = excluded.duration_ms",
            params![uri, kind.as_str(), title, duration_ms, added],
        )?;
        let id: i64 = tx.query_row("SELECT id FROM media_items WHERE uri = ?1", [uri], |r| {
            r.get(0)
        })?;
        tx.execute(
            "INSERT INTO recent (media_id, played_at) VALUES (?1, ?2)
             ON CONFLICT(media_id) DO UPDATE SET played_at = excluded.played_at",
            params![id, added],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn recent(&self, limit: i64) -> Result<Vec<MediaItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.uri, m.kind, m.title, m.duration_ms, m.size_bytes, m.added_at
             FROM recent r JOIN media_items m ON m.id = r.media_id
             ORDER BY r.played_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], Self::media_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_progress(&self, media_id: i64, position_ms: i64, completed: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO watch_progress (media_id, position_ms, completed, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(media_id) DO UPDATE SET
                position_ms = excluded.position_ms,
                completed = excluded.completed,
                updated_at = excluded.updated_at",
            params![media_id, position_ms.max(0), i64::from(completed), now_ms()],
        )?;
        Ok(())
    }

    pub fn progress(&self, media_id: i64) -> Result<Option<WatchProgress>> {
        self.conn
            .query_row(
                "SELECT media_id, position_ms, completed, updated_at FROM watch_progress WHERE media_id = ?1",
                [media_id],
                |row| {
                    Ok(WatchProgress {
                        media_id: row.get(0)?,
                        position_ms: row.get(1)?,
                        completed: row.get::<_, i64>(2)? != 0,
                        updated_at: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn create_playlist(&self, name: &str) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(CoreError::Invalid("playlist name is empty".into()));
        }
        self.conn.execute(
            "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
            params![name.trim(), now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(CoreError::Invalid("playlist name is empty".into()));
        }
        self.conn.execute(
            "UPDATE playlists SET name = ?1 WHERE id = ?2",
            params![name.trim(), id],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM playlists WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, created_at FROM playlists ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn add_to_playlist(&self, playlist_id: i64, media_id: i64) -> Result<()> {
        let next: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?1",
            [playlist_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO playlist_items (playlist_id, media_id, position) VALUES (?1, ?2, ?3)",
            params![playlist_id, media_id, next],
        )?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, playlist_id: i64, media_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1 AND media_id = ?2",
            params![playlist_id, media_id],
        )?;
        Ok(())
    }

    pub fn playlist_items(&self, playlist_id: i64) -> Result<Vec<MediaItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.uri, m.kind, m.title, m.duration_ms, m.size_bytes, m.added_at
             FROM playlist_items p JOIN media_items m ON m.id = p.media_id
             WHERE p.playlist_id = ?1 ORDER BY p.position",
        )?;
        let rows = stmt.query_map([playlist_id], Self::media_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Replaces the persisted queue atomically and stores the current index.
    pub fn save_queue(&self, uris: &[String], current: usize) -> Result<()> {
        // `Connection` is not Sync, so this Store is confined to one thread;
        // unchecked_transaction avoids requiring `&mut self` on every reader.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM queue", [])?;
        {
            let mut stmt = tx.prepare("INSERT INTO queue (position, uri) VALUES (?1, ?2)")?;
            for (i, uri) in uris.iter().enumerate() {
                stmt.execute(params![i64::try_from(i).unwrap_or(i64::MAX), uri])?;
            }
        }
        tx.execute(
            "INSERT INTO queue_meta (key, value) VALUES ('current', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [i64::try_from(current).unwrap_or(0)],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn load_queue(&self) -> Result<(Vec<String>, usize)> {
        let mut stmt = self
            .conn
            .prepare("SELECT uri FROM queue ORDER BY position")?;
        let uris = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let current: i64 = self
            .conn
            .query_row(
                "SELECT value FROM queue_meta WHERE key = 'current'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let current = usize::try_from(current.max(0)).unwrap_or(0);
        let current = current.min(uris.len().saturating_sub(1));
        Ok((uris, current))
    }

    pub fn add_folder(&self, path: &str, recursive: bool) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO folders (path, recursive, last_scan) VALUES (?1, ?2, 0)
             ON CONFLICT(path) DO UPDATE SET recursive = excluded.recursive",
            params![path, i64::from(recursive)],
        )?;
        Ok(self
            .conn
            .query_row("SELECT id FROM folders WHERE path = ?1", [path], |r| {
                r.get(0)
            })?)
    }

    pub fn folders(&self) -> Result<Vec<(i64, String, bool)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, recursive FROM folders ORDER BY path")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn link_folder_media(&self, folder_id: i64, media_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO folder_media (folder_id, media_id) VALUES (?1, ?2)",
            params![folder_id, media_id],
        )?;
        Ok(())
    }

    pub fn add_bookmark(&self, media_id: i64, position_ms: i64, label: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO bookmarks (media_id, position_ms, label, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![media_id, position_ms.max(0), label, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn bookmarks(&self, media_id: i64) -> Result<Vec<Bookmark>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, media_id, position_ms, label, created_at FROM bookmarks
             WHERE media_id = ?1 ORDER BY position_ms",
        )?;
        let rows = stmt.query_map([media_id], |r| {
            Ok(Bookmark {
                id: r.get(0)?,
                media_id: r.get(1)?,
                position_ms: r.get(2)?,
                label: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn delete_bookmark(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn save_preset(&self, name: &str, payload: &str) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(CoreError::Invalid("preset name is empty".into()));
        }
        self.conn.execute(
            "INSERT INTO presets (name, payload, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET payload = excluded.payload",
            params![name.trim(), payload, now_ms()],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM presets WHERE name = ?1",
            [name.trim()],
            |r| r.get(0),
        )?)
    }

    pub fn presets(&self) -> Result<Vec<Preset>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, payload, created_at FROM presets ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Preset {
                id: r.get(0)?,
                name: r.get(1)?,
                payload: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn delete_preset(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM presets WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(uri: &str) -> MediaItem {
        MediaItem {
            id: 0,
            uri: uri.into(),
            kind: MediaKind::Video,
            title: uri.into(),
            duration_ms: 60_000,
            size_bytes: 0,
            added_at: 0,
        }
    }

    #[test]
    fn schema_is_created_on_open() {
        let store = Store::in_memory().unwrap();
        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
        assert!(store.list_media(10, 0).unwrap().is_empty());
    }

    #[test]
    fn upsert_is_keyed_by_uri() {
        let store = Store::in_memory().unwrap();
        let first = store.upsert_media(&media("a.mp4")).unwrap();
        let mut changed = media("a.mp4");
        changed.title = "Renamed".into();
        let second = store.upsert_media(&changed).unwrap();
        assert_eq!(first, second);
        assert_eq!(store.list_media(10, 0).unwrap().len(), 1);
        assert_eq!(store.media_by_id(first).unwrap().unwrap().title, "Renamed");
    }

    #[test]
    fn progress_and_recent_survive_repeated_plays() {
        let store = Store::in_memory().unwrap();
        let id = store
            .record_play("a.mp4", "A", MediaKind::Video, 1000)
            .unwrap();
        store
            .record_play("a.mp4", "A", MediaKind::Video, 1000)
            .unwrap();
        store.save_progress(id, 250, false).unwrap();
        assert_eq!(store.progress(id).unwrap().unwrap().position_ms, 250);
        assert_eq!(store.recent(10).unwrap().len(), 1);
        assert_eq!(store.list_media(10, 0).unwrap().len(), 1);
    }

    #[test]
    fn playlist_crud_keeps_order() {
        let store = Store::in_memory().unwrap();
        let a = store.upsert_media(&media("a.mp4")).unwrap();
        let b = store.upsert_media(&media("b.mp4")).unwrap();
        let pl = store.create_playlist("Show").unwrap();
        store.add_to_playlist(pl, a).unwrap();
        store.add_to_playlist(pl, b).unwrap();
        store.add_to_playlist(pl, a).unwrap();
        let items = store.playlist_items(pl).unwrap();
        assert_eq!(
            items.iter().map(|m| m.id.to_string()).collect::<Vec<_>>(),
            vec![a.to_string(), b.to_string()]
        );
        store.remove_from_playlist(pl, a).unwrap();
        assert_eq!(store.playlist_items(pl).unwrap().len(), 1);
        store.rename_playlist(pl, "Renamed").unwrap();
        assert_eq!(store.playlists().unwrap()[0].name, "Renamed");
        store.delete_playlist(pl).unwrap();
        assert!(store.playlists().unwrap().is_empty());
    }

    #[test]
    fn queue_round_trip() {
        let store = Store::in_memory().unwrap();
        let items = vec!["a.mp4".to_owned(), "b.mp4".to_owned(), "c.mp4".to_owned()];
        store.save_queue(&items, 2).unwrap();
        assert_eq!(store.load_queue().unwrap(), (items.clone(), 2));
        store.save_queue(&items, 99).unwrap();
        assert_eq!(store.load_queue().unwrap().1, 2);
    }

    #[test]
    fn bookmarks_and_presets() {
        let store = Store::in_memory().unwrap();
        let id = store.upsert_media(&media("a.mp4")).unwrap();
        let bm = store.add_bookmark(id, 5000, "Intro").unwrap();
        assert_eq!(store.bookmarks(id).unwrap()[0].label, "Intro");
        store.delete_bookmark(bm).unwrap();
        assert!(store.bookmarks(id).unwrap().is_empty());
        let p = store.save_preset("Show", "{\"audio_delay\":0}").unwrap();
        assert_eq!(store.presets().unwrap()[0].id, p);
        store.delete_preset(p).unwrap();
        assert!(store.presets().unwrap().is_empty());
    }
}
