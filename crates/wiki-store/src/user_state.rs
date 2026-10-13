//! Mutable user state stored separately from immutable Wikipedia snapshots.
//! Initial STATE-001 migration; not yet a backup or history privacy manager.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use wiki_model::ArticleKey;

pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub enum UserStateError {
    Sql(rusqlite::Error),
    UnsupportedSchema(i64),
    InvalidValue,
}
impl From<rusqlite::Error> for UserStateError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

pub struct UserState {
    conn: Connection,
}

/// A visit to an exact article revision, recorded only after explicit history opt-in.
#[derive(Debug)]
pub struct HistoryEntry {
    pub key: ArticleKey,
    pub revision_id: u64,
    pub visited_at: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
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

    pub fn set_bookmark(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        anchor: &str,
    ) -> Result<(), UserStateError> {
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
        let found: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT revision_id,anchor FROM bookmarks WHERE project=?1 AND page_id=?2",
                params![key.project, page],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        found
            .map(|(rev, anchor)| {
                let rev = u64::try_from(rev)
                    .ok()
                    .filter(|id| *id > 0)
                    .ok_or(UserStateError::InvalidValue)?;
                Ok((rev, anchor))
            })
            .transpose()
    }

    /// Persist a revision-scoped scroll position independently of immutable snapshots.
    pub fn set_reading_position(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        scroll_offset: u64,
    ) -> Result<(), UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        let offset = i64::try_from(scroll_offset).map_err(|_| UserStateError::InvalidValue)?;
        self.conn.execute(
            "INSERT INTO reading_positions (project,page_id,revision_id,scroll_offset)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(project,page_id) DO UPDATE SET
             revision_id=excluded.revision_id, scroll_offset=excluded.scroll_offset",
            params![key.project, page, revision, offset],
        )?;
        Ok(())
    }

    /// Returns the saved revision and scroll offset; the caller must verify the
    /// revision before applying an exact position to newly updated content.
    pub fn reading_position(&self, key: &ArticleKey) -> Result<Option<(u64, u64)>, UserStateError> {
        let (page, _) = Self::ids(key, 1)?;
        let found: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT revision_id,scroll_offset FROM reading_positions
                 WHERE project=?1 AND page_id=?2",
                params![key.project, page],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        found
            .map(|(revision, offset)| {
                let revision = u64::try_from(revision)
                    .ok()
                    .filter(|id| *id > 0)
                    .ok_or(UserStateError::InvalidValue)?;
                let offset = u64::try_from(offset).map_err(|_| UserStateError::InvalidValue)?;
                Ok((revision, offset))
            })
            .transpose()
    }

    pub fn remove_bookmark(&self, key: &ArticleKey) -> Result<bool, UserStateError> {
        let (page, _) = Self::ids(key, 1)?;
        Ok(self.conn.execute(
            "DELETE FROM bookmarks WHERE project=?1 AND page_id=?2",
            params![key.project, page],
        )? != 0)
    }

    pub fn remove_reading_position(&self, key: &ArticleKey) -> Result<bool, UserStateError> {
        let (page, _) = Self::ids(key, 1)?;
        Ok(self.conn.execute(
            "DELETE FROM reading_positions WHERE project=?1 AND page_id=?2",
            params![key.project, page],
        )? != 0)
    }

    /// History recording is opt-in; missing or disabled preference means no writes.
    pub fn history_enabled(&self) -> Result<bool, UserStateError> {
        Ok(self.setting("history.enabled")?.as_deref() == Some("true"))
    }

    pub fn set_history_enabled(&self, enabled: bool) -> Result<(), UserStateError> {
        self.set_setting("history.enabled", if enabled { "true" } else { "false" })
    }

    /// Returns false when history is disabled, without creating a visit record.
    pub fn record_visit(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        visited_at: i64,
    ) -> Result<bool, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if visited_at <= 0 {
            return Err(UserStateError::InvalidValue);
        }
        if !self.history_enabled()? {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT INTO history (project,page_id,revision_id,visited_at)
             VALUES (?1,?2,?3,?4)",
            params![key.project, page, revision, visited_at],
        )?;
        Ok(true)
    }

    /// Bounded, newest-first history; invalid persisted identifiers fail closed.
    pub fn recent_history(&self, limit: usize) -> Result<Vec<HistoryEntry>, UserStateError> {
        if limit > 1000 {
            return Err(UserStateError::InvalidValue);
        }
        let mut statement = self.conn.prepare(
            "SELECT project,page_id,revision_id,visited_at FROM history
             ORDER BY visited_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        let mut entries = Vec::new();
        for row in rows {
            let (project, page, revision, visited_at) = row?;
            let page_id = u64::try_from(page).map_err(|_| UserStateError::InvalidValue)?;
            let revision_id = u64::try_from(revision).map_err(|_| UserStateError::InvalidValue)?;
            let key = ArticleKey { project, page_id };
            Self::ids(&key, revision_id)?;
            if visited_at <= 0 {
                return Err(UserStateError::InvalidValue);
            }
            entries.push(HistoryEntry {
                key,
                revision_id,
                visited_at,
            });
        }
        Ok(entries)
    }

    pub fn clear_history(&self) -> Result<usize, UserStateError> {
        Ok(self.conn.execute("DELETE FROM history", [])?)
    }

    /// A chat belongs to one exact article revision; callers must never
    /// silently reuse its messages for a newer revision.
    pub fn create_chat_thread(
        &self,
        key: &ArticleKey,
        revision_id: u64,
    ) -> Result<i64, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        self.conn.execute(
            "INSERT INTO chat_threads (project,page_id,revision_id) VALUES (?1,?2,?3)",
            params![key.project, page, revision],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn check_chat_thread(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        thread_id: i64,
    ) -> Result<(), UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if thread_id <= 0 {
            return Err(UserStateError::InvalidValue);
        }
        let exists: i64 = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_threads
             WHERE id=?1 AND project=?2 AND page_id=?3 AND revision_id=?4)",
            params![thread_id, key.project, page, revision],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(UserStateError::InvalidValue);
        }
        Ok(())
    }

    /// Only human and assistant turns are persisted; provider instructions,
    /// API keys and raw system prompts must not be stored as chat messages.
    pub fn append_chat_message(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        thread_id: i64,
        role: &str,
        content: &str,
    ) -> Result<i64, UserStateError> {
        if !matches!(role, "user" | "assistant")
            || content.is_empty()
            || content.len() > 65536
            || content.contains('\0')
        {
            return Err(UserStateError::InvalidValue);
        }
        self.check_chat_thread(key, revision_id, thread_id)?;
        self.conn.execute(
            "INSERT INTO chat_messages (thread_id,role,content) VALUES (?1,?2,?3)",
            params![thread_id, role, content],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn chat_messages(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        thread_id: i64,
        limit: usize,
    ) -> Result<Vec<ChatMessage>, UserStateError> {
        if limit > 1000 {
            return Err(UserStateError::InvalidValue);
        }
        self.check_chat_thread(key, revision_id, thread_id)?;
        let mut statement = self.conn.prepare(
            "SELECT id,role,content FROM chat_messages
             WHERE thread_id=?1 ORDER BY id ASC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![thread_id, limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Deletion cascades to every message of this exact article-scoped thread.
    pub fn delete_chat_thread(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        thread_id: i64,
    ) -> Result<bool, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if thread_id <= 0 {
            return Err(UserStateError::InvalidValue);
        }
        Ok(self.conn.execute(
            "DELETE FROM chat_threads WHERE id=?1 AND project=?2
             AND page_id=?3 AND revision_id=?4",
            params![thread_id, key.project, page, revision],
        )? != 0)
    }

    /// Never store provider API keys here; use OS secret storage.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), UserStateError> {
        if key.is_empty()
            || key.len() > 128
            || !key.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
            || value.len() > 4096
            || value.contains('\0')
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
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activation::ActiveSnapshotStore;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn key() -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id: 42,
        }
    }

    #[test]
    fn migration_and_non_secret_bookmark_setting_roundtrip() {
        let user = UserState::in_memory().unwrap();
        user.set_bookmark(&key(), 123, "Physics > Origins").unwrap();
        assert_eq!(
            user.bookmark(&key()).unwrap(),
            Some((123, "Physics > Origins".into()))
        );
        user.set_setting("reading.theme", "dark").unwrap();
        assert_eq!(
            user.setting("reading.theme").unwrap().as_deref(),
            Some("dark")
        );
        assert!(matches!(
            user.set_bookmark(&key(), 0, ""),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.set_setting("API_KEY", "secret"),
            Err(UserStateError::InvalidValue)
        ));
    }

    #[test]
    fn reading_position_revision_and_deletion_contract() {
        let user = UserState::in_memory().unwrap();
        assert_eq!(user.reading_position(&key()).unwrap(), None);
        user.set_reading_position(&key(), 123, 0).unwrap();
        assert_eq!(user.reading_position(&key()).unwrap(), Some((123, 0)));
        user.set_reading_position(&key(), 124, 4096).unwrap();
        assert_eq!(user.reading_position(&key()).unwrap(), Some((124, 4096)));
        assert!(matches!(
            user.set_reading_position(&key(), 0, 10),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.set_reading_position(&key(), 124, u64::MAX),
            Err(UserStateError::InvalidValue)
        ));
        assert_eq!(user.reading_position(&key()).unwrap(), Some((124, 4096)));
        assert!(user.remove_reading_position(&key()).unwrap());
        assert!(!user.remove_reading_position(&key()).unwrap());
        assert_eq!(user.reading_position(&key()).unwrap(), None);

        user.set_bookmark(&key(), 124, "Section").unwrap();
        assert!(user.remove_bookmark(&key()).unwrap());
        assert!(!user.remove_bookmark(&key()).unwrap());
        assert_eq!(user.bookmark(&key()).unwrap(), None);
    }

    #[test]
    fn history_is_opt_in_bounded_and_deletable() {
        let user = UserState::in_memory().unwrap();
        assert!(!user.history_enabled().unwrap());
        assert!(!user.record_visit(&key(), 100, 1234).unwrap());
        assert!(user.recent_history(10).unwrap().is_empty());

        user.set_history_enabled(true).unwrap();
        assert!(user.record_visit(&key(), 100, 1234).unwrap());
        assert!(user.record_visit(&key(), 101, 1235).unwrap());
        let visits = user.recent_history(1).unwrap();
        assert_eq!(visits.len(), 1);
        assert_eq!(visits[0].key.page_id, 42);
        assert_eq!(visits[0].revision_id, 101);
        assert_eq!(visits[0].visited_at, 1235);
        assert!(matches!(
            user.record_visit(&key(), 101, 0),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.recent_history(1001),
            Err(UserStateError::InvalidValue)
        ));

        user.set_history_enabled(false).unwrap();
        assert!(!user.record_visit(&key(), 102, 1236).unwrap());
        assert_eq!(user.recent_history(10).unwrap().len(), 2);
        assert_eq!(user.clear_history().unwrap(), 2);
        assert_eq!(user.clear_history().unwrap(), 0);
        assert!(user.recent_history(10).unwrap().is_empty());
    }

    #[test]
    fn chat_threads_are_revision_scoped_bounded_and_cascade_deleted() {
        let user = UserState::in_memory().unwrap();
        let thread = user.create_chat_thread(&key(), 123).unwrap();
        let first = user
            .append_chat_message(&key(), 123, thread, "user", "What is this?")
            .unwrap();
        let second = user
            .append_chat_message(&key(), 123, thread, "assistant", "A test.")
            .unwrap();
        assert!(second > first);
        let messages = user.chat_messages(&key(), 123, thread, 10).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].content, "A test.");
        assert_eq!(user.chat_messages(&key(), 123, thread, 1).unwrap().len(), 1);
        assert!(matches!(
            user.chat_messages(&key(), 124, thread, 10),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.append_chat_message(&key(), 124, thread, "user", "wrong revision"),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.append_chat_message(&key(), 123, thread, "system", "secret"),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.append_chat_message(&key(), 123, thread, "user", &"x".repeat(65537)),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 1001),
            Err(UserStateError::InvalidValue)
        ));
        assert!(!user.delete_chat_thread(&key(), 124, thread).unwrap());
        assert!(user.delete_chat_thread(&key(), 123, thread).unwrap());
        assert!(!user.delete_chat_thread(&key(), 123, thread).unwrap());
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 10),
            Err(UserStateError::InvalidValue)
        ));
    }

    #[test]
    fn state_survives_snapshot_activation_and_process_reopen() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-user-state-{}-{stamp}", std::process::id()));
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
            user.set_reading_position(&key(), 123, 2048).unwrap();
        }
        snapshot.switch_to_prevalidated("new").unwrap();
        let user = UserState::open(&db).unwrap();
        assert_eq!(
            user.bookmark(&key()).unwrap(),
            Some((123, "saved place".into()))
        );
        assert_eq!(
            user.setting("history.enabled").unwrap().as_deref(),
            Some("false")
        );
        assert_eq!(user.reading_position(&key()).unwrap(), Some((123, 2048)));
        assert_eq!(snapshot.current().unwrap().as_deref(), Some("new"));
        drop(user);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unsupported_future_schema() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("PRAGMA user_version=99;").unwrap();
        assert!(matches!(
            UserState::initialize(connection),
            Err(UserStateError::UnsupportedSchema(99))
        ));
    }
}
