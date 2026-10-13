//! Mutable user state stored separately from immutable Wikipedia snapshots.
//! Initial STATE-001 migration; not yet a backup or history privacy manager.

use std::path::Path;
use rusqlite::{params, Connection, OptionalExtension};
use wiki_model::ArticleKey;

pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub enum UserStateError {
    Sql(rusqlite::Error),
    UnsupportedSchema(i64),
    InvalidValue,
}
impl From<rusqlite::Error> for UserStateError {
    fn from(error: rusqlite::Error) -> Self { Self::Sql(error) }
}

pub struct UserState {
    conn: Connection,
}

impl UserState {
    pub fn open(path: &Path) -> Result<Self, UserStateError> {
        Self::initialize(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, UserStateError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut conn: Connection) -> Result<Self, UserStateError> {
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(UserStateError::UnsupportedSchema(version));
        }
        if version == 0 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE bookmarks (
                   project TEXT NOT NULL,
                   page_id INTEGER NOT NULL CHECK(page_id > 0),
                   revision_id INTEGER NOT NULL CHECK(revision_id > 0),
                   anchor TEXT NOT NULL,
                   PRIMARY KEY(project, page_id)
                 );
                 CREATE TABLE reading_positions (
                   project TEXT NOT NULL,
                   page_id INTEGER NOT NULL CHECK(page_id > 0),
                   revision_id INTEGER NOT NULL CHECK(revision_id > 0),
                   scroll_offset INTEGER NOT NULL CHECK(scroll_offset >= 0),
                   PRIMARY KEY(project, page_id)
                 );
                 CREATE TABLE history (
                   id INTEGER PRIMARY KEY,
                   project TEXT NOT NULL,
                   page_id INTEGER NOT NULL,
                   revision_id INTEGER NOT NULL,
                   visited_at INTEGER NOT NULL
                 );
                 CREATE TABLE chat_threads (
                   id INTEGER PRIMARY KEY,
                   project TEXT NOT NULL,
                   page_id INTEGER NOT NULL,
                   revision_id INTEGER NOT NULL
                 );
                 CREATE TABLE chat_messages (
                   id INTEGER PRIMARY KEY,
                   thread_id INTEGER NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
                   role TEXT NOT NULL,
                   content TEXT NOT NULL
                 );
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE pack_owners (
                   pack_id TEXT NOT NULL,
                   digest TEXT NOT NULL,
                   PRIMARY KEY(pack_id, digest)
                 );
                 CREATE TABLE builder_jobs (
                   job_id TEXT PRIMARY KEY,
                   status TEXT NOT NULL
                 );",
            )?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
        Ok(Self { conn })
    }

    fn ids(key: &ArticleKey, revision_id: u64) -> Result<(i64, i64), UserStateError> {
        key.validate().map_err(|_| UserStateError::InvalidValue)?;
        let page_id = i64::try_from(key.page_id).map_err(|_| UserStateError::InvalidValue)?;
        let revision_id = i64::try_from(revision_id).map_err(|_| UserStateError::InvalidValue)?;
        if revision_id == 0 {
            return Err(UserStateError::InvalidValue);
        }
        Ok((page_id, revision_id))
    }

    pub fn set_bookmark(&self, key: &ArticleKey, revision_id: u64, anchor: &str)
        -> Result<(), UserStateError>
    {
        let (page, revision) = Self::ids(key, revision_id)?;
        if anchor.len() > 4096 || anchor.contains('\0') {
            return Err(UserStateError::InvalidValue);
        }
        self.conn.execute(
            "INSERT INTO bookmarks (project,page_id,revision_id,anchor) VALUES (?1,?2,?3,?4)
             ON CONFLICT(project,page_id) DO UPDATE SET
             revision_id=excluded.revision_id, anchor=excluded.anchor",
            params![key.project, page, revision, anchor],
        )?;
        Ok(())
    }

    pub fn bookmark(&self, key: &ArticleKey) -> Result<Option<(u64, String)>, UserStateError> {
        let (page, _) = Self::ids(key, 1)?;
        let found: Option<(i64, String)> = self.conn.query_row(
            "SELECT revision_id,anchor FROM bookmarks WHERE project=?1 AND page_id=?2",
            params![key.project, page],
            |row| Ok((row.get(0)?, row.get(1)?))
        ).optional()?;
        found.map(|(rev, anchor)| {
            let rev = u64::try_from(rev).ok().filter(|id| *id > 0)
                .ok_or(UserStateError::InvalidValue)?;
            Ok((rev, anchor))
        }).transpose()
    }

    /// Never store provider API keys here; use OS secret storage.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), UserStateError> {
        if key.is_empty() || key.len() > 128 || !key.bytes().all(|b|
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
            || value.len() > 4096 || value.contains('\0')
        {
            return Err(UserStateError::InvalidValue);
        }
        self.conn.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, UserStateError> {
        if key.is_empty() || key.len() > 128 {
            return Err(UserStateError::InvalidValue);
        }
        Ok(self.conn.query_row(
            "SELECT value FROM settings WHERE key=?1", params![key], |row| row.get(0)
        ).optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activation::ActiveSnapshotStore;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn key() -> ArticleKey {
        ArticleKey { project: "enwiki".into(), page_id: 42 }
    }

    #[test]
    fn migration_and_non_secret_bookmark_setting_roundtrip() {
        let user = UserState::in_memory().unwrap();
        user.set_bookmark(&key(), 123, "Physics > Origins").unwrap();
        assert_eq!(user.bookmark(&key()).unwrap(), Some((123, "Physics > Origins".into())));
        user.set_setting("reading.theme", "dark").unwrap();
        assert_eq!(user.setting("reading.theme").unwrap().as_deref(), Some("dark"));
        assert!(matches!(user.set_bookmark(&key(), 0, ""), Err(UserStateError::InvalidValue)));
        assert!(matches!(user.set_setting("API_KEY", "secret"), Err(UserStateError::InvalidValue)));
    }

    #[test]
    fn state_survives_snapshot_activation_and_process_reopen() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("wiki-user-state-{}-{stamp}", std::process::id()));
        for id in ["old", "new"] {
            let path = root.join("snapshots").join(id);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("manifest.json"), "{}").unwrap();
        }
        fs::create_dir_all(root.join("user-state")).unwrap();
        let db = root.join("user-state/user.sqlite");
        let snapshot = ActiveSnapshotStore::new(&root);
        snapshot.switch_to_prevalidated("old").unwrap();
        {
            let user = UserState::open(&db).unwrap();
            user.set_bookmark(&key(), 123, "saved place").unwrap();
            user.set_setting("history.enabled", "false").unwrap();
        }
        snapshot.switch_to_prevalidated("new").unwrap();
        let user = UserState::open(&db).unwrap();
        assert_eq!(user.bookmark(&key()).unwrap(), Some((123, "saved place".into())));
        assert_eq!(user.setting("history.enabled").unwrap().as_deref(), Some("false"));
        assert_eq!(snapshot.current().unwrap().as_deref(), Some("new"));
        drop(user);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsupported_future_schema() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("PRAGMA user_version=99;").unwrap();
        assert!(matches!(UserState::initialize(connection), Err(UserStateError::UnsupportedSchema(99))));
    }
}
