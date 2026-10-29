//! Mutable user state stored separately from immutable Wikipedia snapshots.
//! Initial STATE-001 migration; not yet a backup or history privacy manager.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use wiki_model::ArticleKey;

pub const SCHEMA_VERSION: i64 = 2;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderJob {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedBookmark {
    pub key: ArticleKey,
    pub revision_id: u64,
    pub anchor: String,
}

fn valid_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn valid_object_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_job_id(job_id: &str) -> bool {
    !job_id.is_empty()
        && job_id.len() <= 128
        && job_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
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
            transaction.pragma_update(None, "user_version", 1)?;
            transaction.commit()?;
        }
        if version < 2 {
            // Indexes live in mutable user state, never in immutable article
            // snapshots. A single transaction keeps a v1 database recoverable
            // if a migration fails or the process is interrupted.
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                "CREATE INDEX history_recent_idx ON history(visited_at DESC, id DESC);
                 CREATE INDEX chat_threads_article_idx
                   ON chat_threads(project, page_id, revision_id, id DESC);
                 CREATE INDEX chat_messages_thread_idx ON chat_messages(thread_id, id);
                 CREATE INDEX pack_owners_digest_idx ON pack_owners(digest, pack_id);",
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

    /// Enumerate validated bookmarks in deterministic order for management UI.
    /// Canonical article identity must be revalidated when reading mutable rows.
    pub fn list_bookmarks(&self, limit: usize) -> Result<Vec<SavedBookmark>, UserStateError> {
        if limit > 1000 {
            return Err(UserStateError::InvalidValue);
        }
        let mut statement = self.conn.prepare(
            "SELECT project,page_id,revision_id,anchor FROM bookmarks
             ORDER BY project,page_id LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut bookmarks = Vec::new();
        for row in rows {
            let (project, page_id, revision_id, anchor) = row?;
            let page_id = u64::try_from(page_id).map_err(|_| UserStateError::InvalidValue)?;
            let revision_id =
                u64::try_from(revision_id).map_err(|_| UserStateError::InvalidValue)?;
            let key = ArticleKey { project, page_id };
            Self::ids(&key, revision_id)?;
            if anchor.len() > 4096 || anchor.contains('\0') {
                return Err(UserStateError::InvalidValue);
            }
            bookmarks.push(SavedBookmark {
                key,
                revision_id,
                anchor,
            });
        }
        Ok(bookmarks)
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

    /// Return a scroll offset only for the exact displayed revision. A saved
    /// position from an older revision is not a reliable HTML coordinate.
    pub fn reading_position_for_revision(
        &self,
        key: &ArticleKey,
        revision_id: u64,
    ) -> Result<Option<u64>, UserStateError> {
        Self::ids(key, revision_id)?;
        Ok(self
            .reading_position(key)?
            .and_then(|(saved_revision, offset)| (saved_revision == revision_id).then_some(offset)))
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
        // Evaluate consent and insert in one SQLite statement. Separate
        // preference-read and insert calls can race with disabling history.
        Ok(self.conn.execute(
            "INSERT INTO history (project,page_id,revision_id,visited_at)
             SELECT ?1,?2,?3,?4 WHERE EXISTS (
               SELECT 1 FROM settings
               WHERE key='history.enabled' AND value='true'
             )",
            params![key.project, page, revision, visited_at],
        )? != 0)
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
    /// Atomically delete personal reading activity while preserving separately
    /// managed installed-pack ownership, builder jobs and non-secret settings.
    /// SQLite foreign keys cascade chat message deletion from chat threads.
    pub fn clear_personal_reading_data(&mut self) -> Result<(), UserStateError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM bookmarks;
             DELETE FROM reading_positions;
             DELETE FROM history;
             DELETE FROM chat_threads;",
        )?;
        tx.commit()?;
        Ok(())
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

    /// Find saved threads for exactly one page and revision after restart.
    /// A newer revision must never inherit a prior revision's conversation.
    pub fn list_chat_threads(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        limit: usize,
    ) -> Result<Vec<i64>, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if limit > 1000 {
            return Err(UserStateError::InvalidValue);
        }
        let mut statement = self.conn.prepare(
            "SELECT id FROM chat_threads WHERE project=?1 AND page_id=?2
             AND revision_id=?3 ORDER BY id DESC LIMIT ?4",
        )?;
        let rows = statement
            .query_map(params![key.project, page, revision, limit as i64], |row| {
                row.get::<_, i64>(0)
            })?;
        let mut ids = Vec::new();
        for row in rows {
            let id = row?;
            if id <= 0 {
                return Err(UserStateError::InvalidValue);
            }
            ids.push(id);
        }
        Ok(ids)
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
        let messages = rows.collect::<Result<Vec<_>, _>>()?;
        // SQLite rows are mutable and must not be treated as trusted simply
        // because application writes enforce this contract.
        for message in &messages {
            if message.id <= 0
                || !matches!(message.role.as_str(), "user" | "assistant")
                || message.content.is_empty()
                || message.content.len() > 65536
                || message.content.contains('\0')
            {
                return Err(UserStateError::InvalidValue);
            }
        }
        Ok(messages)
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

    /// Record that this installed pack owns one content-addressed object.
    /// Identities are constrained before storing them in mutable user state.
    pub fn add_pack_owner(&self, pack_id: &str, digest: &str) -> Result<bool, UserStateError> {
        if !valid_pack_id(pack_id) || !valid_object_digest(digest) {
            return Err(UserStateError::InvalidValue);
        }
        Ok(self.conn.execute(
            "INSERT OR IGNORE INTO pack_owners (pack_id,digest) VALUES (?1,?2)",
            params![pack_id, digest],
        )? != 0)
    }

    /// Drop a single pack's ownership, never another pack's reference.
    /// Physical object deletion must be handled by a coordinated install
    /// transaction after confirming no remaining owners or other leases.
    pub fn remove_pack_owner(&self, pack_id: &str, digest: &str) -> Result<bool, UserStateError> {
        if !valid_pack_id(pack_id) || !valid_object_digest(digest) {
            return Err(UserStateError::InvalidValue);
        }
        Ok(self.conn.execute(
            "DELETE FROM pack_owners WHERE pack_id=?1 AND digest=?2",
            params![pack_id, digest],
        )? != 0)
    }

    pub fn object_owner_count(&self, digest: &str) -> Result<u64, UserStateError> {
        if !valid_object_digest(digest) {
            return Err(UserStateError::InvalidValue);
        }
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pack_owners WHERE digest=?1",
            params![digest],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| UserStateError::InvalidValue)
    }

    /// Persist a durable, bounded builder-job state independent of snapshots.
    /// The pipeline owns actual recovery and cancellation implementation.
    pub fn set_builder_job_status(&self, job_id: &str, status: &str) -> Result<(), UserStateError> {
        if !valid_job_id(job_id)
            || !matches!(
                status,
                "queued" | "running" | "succeeded" | "failed" | "cancelled"
            )
        {
            return Err(UserStateError::InvalidValue);
        }
        self.conn.execute(
            "INSERT INTO builder_jobs (job_id,status) VALUES (?1,?2)
             ON CONFLICT(job_id) DO UPDATE SET status=excluded.status",
            params![job_id, status],
        )?;
        Ok(())
    }

    pub fn builder_job_status(&self, job_id: &str) -> Result<Option<String>, UserStateError> {
        if !valid_job_id(job_id) {
            return Err(UserStateError::InvalidValue);
        }
        self.conn
            .query_row(
                "SELECT status FROM builder_jobs WHERE job_id=?1",
                params![job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Enumerate bounded persisted jobs after restart without guessing IDs.
    /// Unexpected/corrupted status rows cannot masquerade as valid jobs.
    pub fn list_builder_jobs(&self, limit: usize) -> Result<Vec<BuilderJob>, UserStateError> {
        if limit > 1000 {
            return Err(UserStateError::InvalidValue);
        }
        let mut statement = self
            .conn
            .prepare("SELECT job_id,status FROM builder_jobs ORDER BY job_id ASC LIMIT ?1")?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok(BuilderJob {
                id: row.get(0)?,
                status: row.get(1)?,
            })
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            let job = row?;
            if !valid_job_id(&job.id)
                || !matches!(
                    job.status.as_str(),
                    "queued" | "running" | "succeeded" | "failed" | "cancelled"
                )
            {
                return Err(UserStateError::InvalidValue);
            }
            jobs.push(job);
        }
        Ok(jobs)
    }

    pub fn remove_builder_job(&self, job_id: &str) -> Result<bool, UserStateError> {
        if !valid_job_id(job_id) {
            return Err(UserStateError::InvalidValue);
        }
        Ok(self
            .conn
            .execute("DELETE FROM builder_jobs WHERE job_id=?1", params![job_id])?
            != 0)
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

    pub fn delete_setting(&self, key: &str) -> Result<bool, UserStateError> {
        if key.is_empty()
            || key.len() > 128
            || !key.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
        {
            return Err(UserStateError::InvalidValue);
        }
        Ok(self
            .conn
            .execute("DELETE FROM settings WHERE key=?1", params![key])?
            != 0)
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
        assert!(user.delete_setting("reading.theme").unwrap());
        assert!(!user.delete_setting("reading.theme").unwrap());
        assert_eq!(user.setting("reading.theme").unwrap(), None);
        assert!(matches!(
            user.delete_setting("INVALID_KEY"),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.set_setting("API_KEY", "secret"),
            Err(UserStateError::InvalidValue)
        ));
    }

    #[test]
    fn bookmark_listing_is_bounded_ordered_and_validates_stored_rows() {
        let user = UserState::in_memory().unwrap();
        let earlier = ArticleKey {
            project: "enwiki".into(),
            page_id: 7,
        };
        user.set_bookmark(&key(), 123, "physics").unwrap();
        user.set_bookmark(&earlier, 98, "history").unwrap();
        let bookmarks = user.list_bookmarks(2).unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].key.page_id, 7);
        assert_eq!(bookmarks[0].revision_id, 98);
        assert_eq!(user.list_bookmarks(1).unwrap().len(), 1);
        assert!(matches!(
            user.list_bookmarks(1001),
            Err(UserStateError::InvalidValue)
        ));
        user.conn
            .execute(
                "INSERT INTO bookmarks(project,page_id,revision_id,anchor)
                 VALUES ('BAD',99,123,'bad')",
                [],
            )
            .unwrap();
        assert!(matches!(
            user.list_bookmarks(3),
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
        assert_eq!(
            user.reading_position_for_revision(&key(), 124).unwrap(),
            Some(4096)
        );
        assert_eq!(
            user.reading_position_for_revision(&key(), 123).unwrap(),
            None
        );
        assert!(matches!(
            user.reading_position_for_revision(&key(), 0),
            Err(UserStateError::InvalidValue)
        ));
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
        user.set_setting("history.enabled", "corrupt").unwrap();
        assert!(!user.record_visit(&key(), 103, 1237).unwrap());
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
    fn chat_reader_rejects_corrupted_persisted_roles_and_messages() {
        let user = UserState::in_memory().unwrap();
        let thread = user.create_chat_thread(&key(), 123).unwrap();
        let id = user
            .append_chat_message(&key(), 123, thread, "user", "Hello")
            .unwrap();
        assert_eq!(user.chat_messages(&key(), 123, thread, 5).unwrap().len(), 1);
        user.conn
            .execute(
                "UPDATE chat_messages SET role='system' WHERE id=?1",
                params![id],
            )
            .unwrap();
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 5),
            Err(UserStateError::InvalidValue)
        ));
        user.conn
            .execute(
                "UPDATE chat_messages SET role='user',content='' WHERE id=?1",
                params![id],
            )
            .unwrap();
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 5),
            Err(UserStateError::InvalidValue)
        ));
        user.conn
            .execute(
                "UPDATE chat_messages SET content=?1 WHERE id=?2",
                params!["bad\0value", id],
            )
            .unwrap();
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 5),
            Err(UserStateError::InvalidValue)
        ));
        user.conn
            .execute(
                "UPDATE chat_messages SET content=?1 WHERE id=?2",
                params!["x".repeat(65537), id],
            )
            .unwrap();
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 5),
            Err(UserStateError::InvalidValue)
        ));
        user.conn
            .execute(
                "UPDATE chat_messages SET content='ok',id=-1 WHERE id=?1",
                params![id],
            )
            .unwrap();
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 5),
            Err(UserStateError::InvalidValue)
        ));
    }

    #[test]
    fn shared_object_ownership_survives_removal_of_one_pack() {
        let user = UserState::in_memory().unwrap();
        let digest = "a".repeat(64);
        assert_eq!(user.object_owner_count(&digest).unwrap(), 0);
        assert!(user.add_pack_owner("science", &digest).unwrap());
        assert!(!user.add_pack_owner("science", &digest).unwrap());
        assert!(user.add_pack_owner("mathematics", &digest).unwrap());
        assert_eq!(user.object_owner_count(&digest).unwrap(), 2);
        assert!(user.remove_pack_owner("science", &digest).unwrap());
        assert!(!user.remove_pack_owner("science", &digest).unwrap());
        assert_eq!(user.object_owner_count(&digest).unwrap(), 1);
        assert!(user.remove_pack_owner("mathematics", &digest).unwrap());
        assert_eq!(user.object_owner_count(&digest).unwrap(), 0);
        assert!(matches!(
            user.add_pack_owner("../outside", &digest),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.add_pack_owner("science", &"F".repeat(64)),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.object_owner_count("not-a-digest"),
            Err(UserStateError::InvalidValue)
        ));
    }

    #[test]
    fn durable_job_inventory_tracks_validated_statuses() {
        let user = UserState::in_memory().unwrap();
        assert_eq!(user.builder_job_status("build_01").unwrap(), None);
        user.set_builder_job_status("build_01", "queued").unwrap();
        assert_eq!(
            user.builder_job_status("build_01").unwrap().as_deref(),
            Some("queued")
        );
        user.set_builder_job_status("build_01", "running").unwrap();
        user.set_builder_job_status("build_01", "succeeded")
            .unwrap();
        assert_eq!(
            user.builder_job_status("build_01").unwrap().as_deref(),
            Some("succeeded")
        );
        assert!(matches!(
            user.set_builder_job_status("../outside", "running"),
            Err(UserStateError::InvalidValue)
        ));
        assert!(matches!(
            user.set_builder_job_status("build_01", "arbitrary"),
            Err(UserStateError::InvalidValue)
        ));
        assert_eq!(
            user.builder_job_status("build_01").unwrap().as_deref(),
            Some("succeeded")
        );
        assert!(user.remove_builder_job("build_01").unwrap());
        assert!(!user.remove_builder_job("build_01").unwrap());
        assert_eq!(user.builder_job_status("build_01").unwrap(), None);
    }

    #[test]
    fn personal_data_deletion_is_atomic_and_preserves_installed_pack_ownership() {
        let mut user = UserState::in_memory().unwrap();
        let digest = "d".repeat(64);
        user.add_pack_owner("science", &digest).unwrap();
        user.set_builder_job_status("build_01", "running").unwrap();
        user.set_setting("reading.theme", "dark").unwrap();
        user.set_bookmark(&key(), 123, "physics").unwrap();
        user.set_reading_position(&key(), 123, 240).unwrap();
        user.set_history_enabled(true).unwrap();
        user.record_visit(&key(), 123, 100).unwrap();
        let thread = user.create_chat_thread(&key(), 123).unwrap();
        user.append_chat_message(&key(), 123, thread, "user", "explain")
            .unwrap();

        user.clear_personal_reading_data().unwrap();
        assert_eq!(user.bookmark(&key()).unwrap(), None);
        assert_eq!(user.reading_position(&key()).unwrap(), None);
        assert!(user.recent_history(10).unwrap().is_empty());
        assert!(matches!(
            user.chat_messages(&key(), 123, thread, 10),
            Err(UserStateError::InvalidValue)
        ));
        assert_eq!(user.object_owner_count(&digest).unwrap(), 1);
        assert_eq!(
            user.builder_job_status("build_01").unwrap().as_deref(),
            Some("running")
        );
        assert_eq!(
            user.setting("reading.theme").unwrap().as_deref(),
            Some("dark")
        );
        user.clear_personal_reading_data().unwrap();
    }

    #[test]
    fn job_inventory_is_ordered_bounded_and_rejects_corrupt_status() {
        let user = UserState::in_memory().unwrap();
        user.set_builder_job_status("job_b", "running").unwrap();
        user.set_builder_job_status("job_a", "queued").unwrap();
        assert_eq!(user.list_builder_jobs(1).unwrap()[0].id, "job_a");
        assert_eq!(user.list_builder_jobs(2).unwrap().len(), 2);
        assert!(matches!(
            user.list_builder_jobs(1001),
            Err(UserStateError::InvalidValue)
        ));
        user.conn
            .execute(
                "UPDATE builder_jobs SET status='unknown' WHERE job_id='job_a'",
                [],
            )
            .unwrap();
        assert!(matches!(
            user.list_builder_jobs(2),
            Err(UserStateError::InvalidValue)
        ));
    }

    #[test]
    fn chat_thread_discovery_is_revision_scoped_and_bounded() {
        let user = UserState::in_memory().unwrap();
        let older = user.create_chat_thread(&key(), 123).unwrap();
        let newest = user.create_chat_thread(&key(), 123).unwrap();
        let another = user.create_chat_thread(&key(), 124).unwrap();
        assert_eq!(
            user.list_chat_threads(&key(), 123, 2).unwrap(),
            vec![newest, older]
        );
        assert_eq!(
            user.list_chat_threads(&key(), 124, 2).unwrap(),
            vec![another]
        );
        assert_eq!(
            user.list_chat_threads(&key(), 123, 1).unwrap(),
            vec![newest]
        );
        assert!(matches!(
            user.list_chat_threads(&key(), 123, 1001),
            Err(UserStateError::InvalidValue)
        ));
        user.delete_chat_thread(&key(), 123, older).unwrap();
        assert_eq!(
            user.list_chat_threads(&key(), 123, 2).unwrap(),
            vec![newest]
        );
    }

    #[test]
    fn opt_in_history_and_disabled_preference_survive_reopen() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-history-reopen-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("user.sqlite");
        {
            let state = UserState::open(&db).unwrap();
            assert!(!state.history_enabled().unwrap());
            assert!(!state.record_visit(&key(), 123, 1700000000).unwrap());
            state.set_history_enabled(true).unwrap();
            assert!(state.record_visit(&key(), 123, 1700000001).unwrap());
        }
        {
            let state = UserState::open(&db).unwrap();
            assert!(state.history_enabled().unwrap());
            let rows = state.recent_history(10).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].revision_id, 123);
            assert_eq!(rows[0].visited_at, 1700000001);
            state.set_history_enabled(false).unwrap();
            assert!(!state.record_visit(&key(), 124, 1700000002).unwrap());
        }
        {
            let state = UserState::open(&db).unwrap();
            assert!(!state.history_enabled().unwrap());
            assert_eq!(state.recent_history(10).unwrap().len(), 1);
            assert_eq!(state.clear_history().unwrap(), 1);
        }
        fs::remove_dir_all(root).unwrap();
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
    fn migrates_v1_in_place_without_losing_mutable_state() {
        let old = UserState::in_memory().unwrap();
        old.set_bookmark(&key(), 123, "Saved").unwrap();
        old.set_reading_position(&key(), 123, 1234).unwrap();
        old.set_history_enabled(true).unwrap();
        old.record_visit(&key(), 123, 1_700_000_000).unwrap();
        let thread = old.create_chat_thread(&key(), 123).unwrap();
        old.append_chat_message(&key(), 123, thread, "user", "Hello")
            .unwrap();
        old.add_pack_owner("physics", &"a".repeat(64)).unwrap();
        old.set_builder_job_status("job_1", "queued").unwrap();

        // Reconstruct the exact v1 schema (before the four v2 indexes) with
        // existing data, as would be found after upgrading an older install.
        old.conn
            .execute_batch(
                "DROP INDEX history_recent_idx;
                 DROP INDEX chat_threads_article_idx;
                 DROP INDEX chat_messages_thread_idx;
                 DROP INDEX pack_owners_digest_idx;
                 PRAGMA user_version=1;",
            )
            .unwrap();
        let upgraded = UserState::initialize(old.conn).unwrap();
        let version: i64 = upgraded
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        for name in [
            "history_recent_idx",
            "chat_threads_article_idx",
            "chat_messages_thread_idx",
            "pack_owners_digest_idx",
        ] {
            let count: i64 = upgraded
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    params![name],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing migrated index: {name}");
        }
        assert_eq!(
            upgraded.bookmark(&key()).unwrap(),
            Some((123, "Saved".into()))
        );
        assert_eq!(
            upgraded.reading_position_for_revision(&key(), 123).unwrap(),
            Some(1234)
        );
        assert_eq!(upgraded.recent_history(10).unwrap().len(), 1);
        assert_eq!(
            upgraded
                .chat_messages(&key(), 123, thread, 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(upgraded.object_owner_count(&"a".repeat(64)).unwrap(), 1);
        assert_eq!(
            upgraded.builder_job_status("job_1").unwrap().as_deref(),
            Some("queued")
        );
        // Reopening an already migrated database must not recreate indexes.
        assert!(UserState::initialize(upgraded.conn).is_ok());
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
