//! Mutable user state stored separately from immutable Wikipedia snapshots.
//! Initial STATE-001 migration; not yet a backup or history privacy manager.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::time::Duration;
use wiki_model::ArticleKey;

pub const SCHEMA_VERSION: i64 = 2;

const PERSONAL_GENERATION_KEY: &str = "internal.personal_generation";

#[derive(Debug)]
pub enum UserStateError {
    Sql(rusqlite::Error),
    Io(io::Error),
    UnsupportedSchema(i64),
    InvalidValue,
}
impl From<rusqlite::Error> for UserStateError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}
impl From<io::Error> for UserStateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
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

/* Check structural contracts before trusting a claimed user-state version.
 * Presence of a table or index name alone does not prove it has the columns
 * expected by readers and migrations. Reject executable schema additions. */
fn validate_schema_shape(conn: &Connection, version: i64) -> Result<(), UserStateError> {
    const TABLE_COLUMNS: &[(&str, &[(&str, &str)])] = &[
        (
            "bookmarks",
            &[
                ("project", "TEXT"),
                ("page_id", "INTEGER"),
                ("revision_id", "INTEGER"),
                ("anchor", "TEXT"),
            ],
        ),
        (
            "reading_positions",
            &[
                ("project", "TEXT"),
                ("page_id", "INTEGER"),
                ("revision_id", "INTEGER"),
                ("scroll_offset", "INTEGER"),
            ],
        ),
        (
            "history",
            &[
                ("id", "INTEGER"),
                ("project", "TEXT"),
                ("page_id", "INTEGER"),
                ("revision_id", "INTEGER"),
                ("visited_at", "INTEGER"),
            ],
        ),
        (
            "chat_threads",
            &[
                ("id", "INTEGER"),
                ("project", "TEXT"),
                ("page_id", "INTEGER"),
                ("revision_id", "INTEGER"),
            ],
        ),
        (
            "chat_messages",
            &[
                ("id", "INTEGER"),
                ("thread_id", "INTEGER"),
                ("role", "TEXT"),
                ("content", "TEXT"),
            ],
        ),
        ("settings", &[("key", "TEXT"), ("value", "TEXT")]),
        ("pack_owners", &[("pack_id", "TEXT"), ("digest", "TEXT")]),
        ("builder_jobs", &[("job_id", "TEXT"), ("status", "TEXT")]),
    ];
    for &(table, expected) in TABLE_COLUMNS {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![table],
            |row| row.get(0),
        )?;
        if count != 1 {
            return Err(UserStateError::InvalidValue);
        }
        let mut statement = conn.prepare(
            "SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid",
        )?;
        let actual = statement
            .query_map(params![table], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if actual.len() != expected.len() {
            return Err(UserStateError::InvalidValue);
        }
        for ((name, column_type, not_null, primary_key), &(expected_name, expected_type)) in
            actual.iter().zip(expected.iter())
        {
            let expected_primary_key = match (table, name.as_str()) {
                ("bookmarks" | "reading_positions", "project") => 1,
                ("bookmarks" | "reading_positions", "page_id") => 2,
                ("pack_owners", "pack_id") => 1,
                ("pack_owners", "digest") => 2,
                ("history" | "chat_threads" | "chat_messages", "id") => 1,
                ("settings", "key") | ("builder_jobs", "job_id") => 1,
                _ => 0,
            };
            // SQLite reports NOT NULL=0 for an INTEGER PRIMARY KEY and for
            // legacy TEXT PRIMARY KEY columns unless explicitly declared.
            let expected_not_null = if matches!(
                (table, name.as_str()),
                ("history" | "chat_threads" | "chat_messages", "id")
                    | ("settings", "key")
                    | ("builder_jobs", "job_id")
            ) {
                0
            } else {
                1
            };
            if name != expected_name
                || !column_type.eq_ignore_ascii_case(expected_type)
                || *not_null != expected_not_null
                || *primary_key != expected_primary_key
            {
                return Err(UserStateError::InvalidValue);
            }
        }
    }
    let unexpected: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('trigger', 'view')",
        [],
        |row| row.get(0),
    )?;
    if unexpected != 0 {
        return Err(UserStateError::InvalidValue);
    }
    let cascade: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_list('chat_messages')
         WHERE \"table\"='chat_threads' AND \"from\"='thread_id'
           AND \"to\"='id' AND on_delete='CASCADE'",
        [],
        |row| row.get(0),
    )?;
    if cascade != 1 {
        return Err(UserStateError::InvalidValue);
    }
    if version >= 2 {
        // Name-only checks can accept a forged index with different query
        // semantics. Validate the owning table, keys, collation and direction.
        type ExpectedIndex = (&'static str, &'static str, &'static [(&'static str, i64)]);
        const INDEXES: &[ExpectedIndex] = &[
            (
                "history",
                "history_recent_idx",
                &[("visited_at", 1), ("id", 1)],
            ),
            (
                "chat_threads",
                "chat_threads_article_idx",
                &[
                    ("project", 0),
                    ("page_id", 0),
                    ("revision_id", 0),
                    ("id", 1),
                ],
            ),
            (
                "chat_messages",
                "chat_messages_thread_idx",
                &[("thread_id", 0), ("id", 0)],
            ),
            (
                "pack_owners",
                "pack_owners_digest_idx",
                &[("digest", 0), ("pack_id", 0)],
            ),
        ];
        for &(table, index, expected_keys) in INDEXES {
            let metadata: Option<(i64, String, i64)> = conn
                .query_row(
                    "SELECT \"unique\", origin, partial FROM pragma_index_list(?1)
                     WHERE name=?2",
                    params![table, index],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            if metadata != Some((0, "c".to_owned(), 0)) {
                return Err(UserStateError::InvalidValue);
            }
            let mut statement = conn.prepare(
                "SELECT name, \"desc\", coll, cid FROM pragma_index_xinfo(?1)
                 WHERE \"key\"=1 ORDER BY seqno",
            )?;
            let actual_keys = statement
                .query_map(params![index], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if actual_keys.len() != expected_keys.len()
                || !actual_keys.iter().zip(expected_keys.iter()).all(
                    |(
                        (name, descending, collation, column_id),
                        &(expected_name, expected_desc),
                    )| {
                        name.as_deref() == Some(expected_name)
                            && *descending == expected_desc
                            && collation == "BINARY"
                            && *column_id >= 0
                    },
                )
            {
                return Err(UserStateError::InvalidValue);
            }
        }
    }
    Ok(())
}

// SQLite integrity_check/quick_check do not consistently report violated
// CHECK constraints across supported SQLite builds. Inspect the persisted
// values explicitly before trusting a database or publishing a restored copy.
fn validate_persisted_checks(conn: &Connection) -> Result<(), UserStateError> {
    for query in [
        "SELECT 1 FROM bookmarks WHERE page_id <= 0 OR revision_id <= 0 LIMIT 1",
        "SELECT 1 FROM reading_positions WHERE page_id <= 0 OR revision_id <= 0 OR scroll_offset < 0 LIMIT 1",
        "SELECT 1 FROM history WHERE id <= 0 OR page_id <= 0 OR revision_id <= 0 OR visited_at <= 0 LIMIT 1",
        "SELECT 1 FROM chat_threads WHERE id <= 0 OR page_id <= 0 OR revision_id <= 0 LIMIT 1",
        "SELECT 1 FROM chat_messages WHERE id <= 0 OR thread_id <= 0
           OR role NOT IN ('user', 'assistant')
           OR length(CAST(content AS BLOB)) NOT BETWEEN 1 AND 65536
           OR instr(content, char(0)) > 0 LIMIT 1",
        "SELECT 1 FROM bookmarks WHERE length(CAST(anchor AS BLOB)) > 4096
           OR instr(anchor, char(0)) > 0 LIMIT 1",
    ] {
        let invalid: Option<i64> = conn.query_row(query, [], |row| row.get(0)).optional()?;
        if invalid.is_some() {
            return Err(UserStateError::InvalidValue);
        }
    }
    // SQLite can report BLOB lengths without materializing arbitrarily large
    // forged TEXT values into Rust Strings. Bound them before row decoding.
    for query in [
        "SELECT 1 FROM pack_owners WHERE length(CAST(pack_id AS BLOB)) > 128
           OR length(CAST(digest AS BLOB)) > 64 LIMIT 1",
        "SELECT 1 FROM builder_jobs WHERE length(CAST(job_id AS BLOB)) > 128
           OR length(CAST(status AS BLOB)) > 16 LIMIT 1",
        "SELECT 1 FROM settings WHERE length(CAST(key AS BLOB)) > 128
           OR length(CAST(value AS BLOB)) > 4096 LIMIT 1",
    ] {
        let invalid: Option<i64> = conn.query_row(query, [], |row| row.get(0)).optional()?;
        if invalid.is_some() {
            return Err(UserStateError::InvalidValue);
        }
    }
    // Check the same canonical identity rules on existing rows that write
    // APIs enforce. A limited list query can otherwise miss a poisoned row.
    for table in ["bookmarks", "reading_positions", "history", "chat_threads"] {
        let size_query =
            format!("SELECT 1 FROM {table} WHERE length(CAST(project AS BLOB)) > 255 LIMIT 1");
        let oversized: Option<i64> = conn
            .query_row(&size_query, [], |row| row.get(0))
            .optional()?;
        if oversized.is_some() {
            return Err(UserStateError::InvalidValue);
        }
        let query = format!("SELECT project,page_id,revision_id FROM {table}");
        let mut statement = conn.prepare(&query)?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (project, page_id, revision_id) = row?;
            let page_id = u64::try_from(page_id).map_err(|_| UserStateError::InvalidValue)?;
            let key = ArticleKey { project, page_id };
            if key.validate().is_err() || revision_id <= 0 {
                return Err(UserStateError::InvalidValue);
            }
        }
    }

    for table in ["pack_owners", "builder_jobs", "settings"] {
        let query = match table {
            "pack_owners" => "SELECT pack_id,digest FROM pack_owners",
            "builder_jobs" => "SELECT job_id,status FROM builder_jobs",
            _ => "SELECT key,value FROM settings",
        };
        let mut statement = conn.prepare(query)?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (name, value) = row?;
            let valid = match table {
                "pack_owners" => valid_pack_id(&name) && valid_object_digest(&value),
                "builder_jobs" => {
                    valid_job_id(&name)
                        && matches!(
                            value.as_str(),
                            "queued" | "running" | "succeeded" | "failed" | "cancelled"
                        )
                }
                _ => {
                    !name.is_empty()
                        && name.len() <= 128
                        && name.bytes().all(|b| {
                            b.is_ascii_lowercase()
                                || b.is_ascii_digit()
                                || matches!(b, b'.' | b'_' | b'-')
                        })
                        && value.len() <= 4096
                        && !value.contains('\0')
                }
            };
            if !valid {
                return Err(UserStateError::InvalidValue);
            }
        }
    }

    Ok(())
}

// Verify the exact standalone SQLite image that will be handed to the user.
// The live source can change while VACUUM INTO runs; source-only preflight
// cannot establish the integrity of the independently copied generation.
fn validate_standalone_copy(path: &Path, expected_schema: i64) -> Result<(), UserStateError> {
    let copy = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let schema: i64 = copy.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if schema != expected_schema {
        return Err(UserStateError::InvalidValue);
    }
    let integrity: String = copy.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(UserStateError::InvalidValue);
    }
    let orphan: Option<i64> = copy
        .query_row(
            "SELECT 1 FROM pragma_foreign_key_check LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if orphan.is_some() {
        return Err(UserStateError::InvalidValue);
    }
    validate_schema_shape(&copy, schema)?;
    validate_persisted_checks(&copy)?;
    Ok(())
}

impl UserState {
    pub fn open(path: &Path) -> Result<Self, UserStateError> {
        let conn = Connection::open(path)?;
        // A second process may hold a short write transaction. Retry rather
        // than treating ordinary concurrent state updates as database errors.
        conn.busy_timeout(Duration::from_secs(5))?;
        let state = Self::initialize(conn)?;
        // Disk-backed personal state supports concurrent readers while another
        // process writes. In-memory state intentionally retains its own mode.
        let mode: String = state
            .conn
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if mode != "wal" {
            return Err(UserStateError::InvalidValue);
        }
        Ok(state)
    }

    pub fn in_memory() -> Result<Self, UserStateError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    /// Write a consistent, standalone SQLite backup of mutable personal state.
    /// The destination must be new, in a caller-owned private directory. It
    /// must never be placed inside a published Wikipedia snapshot or pack.
    /// VACUUM INTO includes committed WAL changes without copying a live WAL
    /// file, and the backup file is synced before success is reported.
    pub fn backup_to_path(&self, destination: &Path) -> Result<(), UserStateError> {
        let destination_text = destination
            .to_str()
            .filter(|text| !text.is_empty())
            .ok_or(UserStateError::InvalidValue)?;
        // A standalone v1 backup is supported and migrates only on a later
        // open. Audit the copied schema generation rather than requiring v2.
        let source_schema: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(1..=SCHEMA_VERSION).contains(&source_schema) {
            return Err(UserStateError::UnsupportedSchema(source_schema));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // Atomic create_new refuses existing files and symlinks, including a
        // pre-existing database that must never be overwritten by a backup.
        let file = options.open(destination)?;
        let result = (|| -> Result<(), UserStateError> {
            self.conn
                .execute("VACUUM main INTO ?1", params![destination_text])?;
            file.sync_all()?;
            validate_standalone_copy(destination, source_schema)?;
            Ok(())
        })();
        drop(file);
        if result.is_err() {
            let _ = fs::remove_file(destination);
        }
        result
    }

    /// Restore a vetted SQLite backup to a NEW user-state database path.
    ///
    /// The source is opened read-only and audited before creating the target.
    /// This never replaces a current user database and never touches installed
    /// content or pack snapshots. Caller must prevent concurrent backup edits
    /// and choose a trusted, private destination directory.
    pub fn restore_backup_to_new_path(
        backup: &Path,
        destination: &Path,
    ) -> Result<(), UserStateError> {
        Self::restore_backup_with_preflight_hook(backup, destination, || {})
    }

    // A deterministic test hook exercises an external writer changing the
    // source after preflight but before VACUUM INTO takes its own snapshot.
    fn restore_backup_with_preflight_hook(
        backup: &Path,
        destination: &Path,
        after_preflight: impl FnOnce(),
    ) -> Result<(), UserStateError> {
        let destination_text = destination
            .to_str()
            .filter(|text| !text.is_empty())
            .ok_or(UserStateError::InvalidValue)?;
        let backup_meta = fs::symlink_metadata(backup)?;
        if !backup_meta.is_file() || backup_meta.file_type().is_symlink() {
            return Err(UserStateError::InvalidValue);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if backup_meta.nlink() != 1 {
                return Err(UserStateError::InvalidValue);
            }
        }
        let source =
            Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        // Never run a migration or initialize a blank, unrelated SQLite file
        // while deciding whether it is safe to restore.
        let schema: i64 = source.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(1..=SCHEMA_VERSION).contains(&schema) {
            return Err(UserStateError::UnsupportedSchema(schema));
        }
        let integrity: String = source.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(UserStateError::InvalidValue);
        }
        let foreign_key_violation: Option<i64> = source
            .query_row(
                "SELECT 1 FROM pragma_foreign_key_check LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if foreign_key_violation.is_some() {
            return Err(UserStateError::InvalidValue);
        }
        validate_schema_shape(&source, schema)?;
        validate_persisted_checks(&source)?;
        after_preflight();

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(destination)?;
        let result = (|| -> Result<(), UserStateError> {
            source.execute("VACUUM main INTO ?1", params![destination_text])?;
            file.sync_all()?;
            // The copy must remain a valid, self-contained database after
            // closing SQLite's write transaction.
            validate_standalone_copy(destination, schema)?;
            Ok(())
        })();
        drop(file);
        if result.is_err() {
            let _ = fs::remove_file(destination);
        }
        result
    }

    fn initialize(mut conn: Connection) -> Result<Self, UserStateError> {
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        // Hold the shared writer lock across version discovery, validation and
        // migrations so concurrent processes cannot act on a stale schema.
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(UserStateError::UnsupportedSchema(version));
        }
        if version > 0 {
            // Audit persisted rows before any v1 migration or user-state read.
            // SQLite quick_check also detects violated CHECK constraints; it
            // does not detect orphaned foreign keys, so check those separately.
            let integrity: String =
                transaction.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
            if integrity != "ok" {
                return Err(UserStateError::InvalidValue);
            }
            let orphan: Option<i64> = transaction
                .query_row(
                    "SELECT 1 FROM pragma_foreign_key_check LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if orphan.is_some() {
                return Err(UserStateError::InvalidValue);
            }
        }
        if version == 0 {
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
        }
        if version == 1 {
            validate_schema_shape(&transaction, version)?;
            validate_persisted_checks(&transaction)?;
        }
        if version < 2 {
            // Indexes live in mutable user state, never in immutable article
            // snapshots. A single transaction keeps a v1 database recoverable
            // if a migration fails or the process is interrupted.
            transaction.execute_batch(
                "CREATE INDEX history_recent_idx ON history(visited_at DESC, id DESC);
                 CREATE INDEX chat_threads_article_idx
                   ON chat_threads(project, page_id, revision_id, id DESC);
                 CREATE INDEX chat_messages_thread_idx ON chat_messages(thread_id, id);
                 CREATE INDEX pack_owners_digest_idx ON pack_owners(digest, pack_id);",
            )?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        // Validate even a claimed-current schema before exposing user state.
        validate_schema_shape(&transaction, SCHEMA_VERSION)?;
        validate_persisted_checks(&transaction)?;
        transaction.commit()?;
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
    /// Apply a scroll-position update only if no cross-process privacy reset
    /// has occurred since the request captured its generation.
    pub fn set_reading_position_for_generation(
        &self,
        generation: u64,
        key: &ArticleKey,
        revision_id: u64,
        scroll_offset: u64,
    ) -> Result<bool, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        let offset = i64::try_from(scroll_offset).map_err(|_| UserStateError::InvalidValue)?;
        if generation > i64::MAX as u64 {
            return Err(UserStateError::InvalidValue);
        }
        let changed = self.conn.execute(
            "INSERT INTO reading_positions(project,page_id,revision_id,scroll_offset)
             SELECT ?1,?2,?3,?4
             WHERE COALESCE(
                 (SELECT value FROM settings WHERE key='internal.personal_generation'), '0'
             )=?5
             ON CONFLICT(project,page_id) DO UPDATE SET
                 revision_id=excluded.revision_id,scroll_offset=excluded.scroll_offset",
            params![key.project, page, revision, offset, generation.to_string()],
        )?;
        Ok(changed != 0)
    }

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

    /// Durable privacy fence shared by all handles of this user-state DB.
    /// A queued operation captures this token when the user initiates it, and
    /// must supply it to a guarded write after any asynchronous work finishes.
    pub fn personal_generation(&self) -> Result<u64, UserStateError> {
        Self::generation_from(&self.conn)
    }

    fn generation_from(conn: &Connection) -> Result<u64, UserStateError> {
        let saved: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![PERSONAL_GENERATION_KEY],
                |row| row.get(0),
            )
            .optional()?;
        match saved {
            None => Ok(0),
            Some(value) => {
                let number = value
                    .parse::<u64>()
                    .map_err(|_| UserStateError::InvalidValue)?;
                if number > i64::MAX as u64 || value != number.to_string() {
                    return Err(UserStateError::InvalidValue);
                }
                Ok(number)
            }
        }
    }

    /// A database-atomic, exact-generation bookmark upsert. Stale queued
    /// writes cannot recreate personal data deleted by another process.
    pub fn set_bookmark_for_generation(
        &self,
        generation: u64,
        key: &ArticleKey,
        revision_id: u64,
        anchor: &str,
    ) -> Result<bool, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if generation > i64::MAX as u64 || anchor.len() > 4096 || anchor.contains('\0') {
            return Err(UserStateError::InvalidValue);
        }
        let changed = self.conn.execute(
            "INSERT INTO bookmarks(project,page_id,revision_id,anchor)
             SELECT ?1,?2,?3,?4
             WHERE COALESCE(
                 (SELECT value FROM settings WHERE key='internal.personal_generation'), '0'
             )=?5
             ON CONFLICT(project,page_id) DO UPDATE SET
                 revision_id=excluded.revision_id,anchor=excluded.anchor",
            params![key.project, page, revision, anchor, generation.to_string()],
        )?;
        Ok(changed != 0)
    }

    /// History opt-in and the durable privacy generation are checked in the
    /// same SQLite statement that inserts the visit, across DB connections.
    pub fn record_visit_for_generation(
        &self,
        generation: u64,
        key: &ArticleKey,
        revision_id: u64,
        visited_at: i64,
    ) -> Result<bool, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if generation > i64::MAX as u64 || visited_at <= 0 {
            return Err(UserStateError::InvalidValue);
        }
        let changed = self.conn.execute(
            "INSERT INTO history(project,page_id,revision_id,visited_at)
             SELECT ?1,?2,?3,?4
             WHERE EXISTS (
                 SELECT 1 FROM settings
                 WHERE key='history.enabled' AND value='true'
             )
             AND COALESCE(
                 (SELECT value FROM settings WHERE key='internal.personal_generation'), '0'
             )=?5",
            params![
                key.project,
                page,
                revision,
                visited_at,
                generation.to_string()
            ],
        )?;
        Ok(changed != 0)
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
        // Serialize the persistent epoch bump and deletion against writes from
        // other SQLite connections. A previously captured token is invalidated
        // before those connections can commit a guarded personal-data write.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = Self::generation_from(&tx)?;
        let next = generation
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(UserStateError::InvalidValue)?;
        tx.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![PERSONAL_GENERATION_KEY, next.to_string()],
        )?;
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

    /// A queued chat-thread creation must not resurrect personal activity
    /// after another process has cleared reading data.
    pub fn create_chat_thread_for_generation(
        &self,
        generation: u64,
        key: &ArticleKey,
        revision_id: u64,
    ) -> Result<Option<i64>, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if generation > i64::MAX as u64 {
            return Err(UserStateError::InvalidValue);
        }
        let changed = self.conn.execute(
            "INSERT INTO chat_threads(project,page_id,revision_id)
             SELECT ?1,?2,?3
             WHERE COALESCE(
                 (SELECT value FROM settings WHERE key='internal.personal_generation'), '0'
             )=?4",
            params![key.project, page, revision, generation.to_string()],
        )?;
        Ok((changed != 0).then(|| self.conn.last_insert_rowid()))
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

    /// Append a revision-scoped chat message only while the captured privacy
    /// generation remains current. Thread identity and generation are checked
    /// in the exact SQLite statement performing the insert.
    pub fn append_chat_message_for_generation(
        &self,
        generation: u64,
        key: &ArticleKey,
        revision_id: u64,
        thread_id: i64,
        role: &str,
        content: &str,
    ) -> Result<Option<i64>, UserStateError> {
        let (page, revision) = Self::ids(key, revision_id)?;
        if generation > i64::MAX as u64
            || thread_id <= 0
            || !matches!(role, "user" | "assistant")
            || content.is_empty()
            || content.len() > 65536
            || content.contains('\0')
        {
            return Err(UserStateError::InvalidValue);
        }
        let changed = self.conn.execute(
            "INSERT INTO chat_messages(thread_id,role,content)
             SELECT id,?1,?2 FROM chat_threads
             WHERE id=?3 AND project=?4 AND page_id=?5 AND revision_id=?6
             AND COALESCE(
                 (SELECT value FROM settings WHERE key='internal.personal_generation'), '0'
             )=?7",
            params![
                role,
                content,
                thread_id,
                key.project,
                page,
                revision,
                generation.to_string()
            ],
        )?;
        Ok((changed != 0).then(|| self.conn.last_insert_rowid()))
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
        if key == PERSONAL_GENERATION_KEY
            || key.is_empty()
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
        if key == PERSONAL_GENERATION_KEY
            || key.is_empty()
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
    fn cross_handle_personal_generation_rejects_stale_queued_writes() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-generation-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("user.sqlite");
        let mut owner = UserState::open(&database).unwrap();
        let other = UserState::open(&database).unwrap();
        owner.set_history_enabled(true).unwrap();
        owner.add_pack_owner("science", &"a".repeat(64)).unwrap();

        let old = other.personal_generation().unwrap();
        assert_eq!(old, 0);
        assert!(other
            .set_bookmark_for_generation(old, &key(), 123, "before")
            .unwrap());
        assert!(other
            .record_visit_for_generation(old, &key(), 123, 100)
            .unwrap());
        owner.clear_personal_reading_data().unwrap();
        assert_eq!(owner.personal_generation().unwrap(), 1);
        assert!(!other
            .set_bookmark_for_generation(old, &key(), 123, "stale")
            .unwrap());
        assert!(!other
            .record_visit_for_generation(old, &key(), 123, 101)
            .unwrap());
        assert!(!other
            .set_reading_position_for_generation(old, &key(), 123, 700)
            .unwrap());
        assert_eq!(
            other
                .create_chat_thread_for_generation(old, &key(), 123)
                .unwrap(),
            None
        );
        assert!(other.bookmark(&key()).unwrap().is_none());
        assert!(other.reading_position(&key()).unwrap().is_none());
        assert!(other.list_chat_threads(&key(), 123, 10).unwrap().is_empty());
        assert_eq!(
            other
                .append_chat_message_for_generation(old, &key(), 123, 1, "user", "stale")
                .unwrap(),
            None
        );
        assert!(other.recent_history(10).unwrap().is_empty());

        let current = other.personal_generation().unwrap();
        assert_eq!(current, 1);
        assert!(other
            .set_bookmark_for_generation(current, &key(), 124, "fresh")
            .unwrap());
        assert!(other
            .record_visit_for_generation(current, &key(), 124, 102)
            .unwrap());
        assert!(other
            .set_reading_position_for_generation(current, &key(), 124, 800)
            .unwrap());
        let thread = other
            .create_chat_thread_for_generation(current, &key(), 124)
            .unwrap()
            .expect("fresh generation creates a chat");
        assert!(thread > 0);
        assert_eq!(other.bookmark(&key()).unwrap(), Some((124, "fresh".into())));
        assert_eq!(other.reading_position(&key()).unwrap(), Some((124, 800)));
        assert_eq!(
            other.list_chat_threads(&key(), 124, 10).unwrap(),
            vec![thread]
        );
        assert!(other
            .append_chat_message_for_generation(current, &key(), 124, thread, "user", "fresh")
            .unwrap()
            .is_some());
        assert_eq!(
            other.chat_messages(&key(), 124, thread, 10).unwrap().len(),
            1
        );
        assert_eq!(other.recent_history(10).unwrap().len(), 1);
        assert_eq!(other.object_owner_count(&"a".repeat(64)).unwrap(), 1);

        for operation in [
            owner.set_setting(PERSONAL_GENERATION_KEY, "0"),
            owner.delete_setting(PERSONAL_GENERATION_KEY).map(|_| ()),
        ] {
            assert!(matches!(operation, Err(UserStateError::InvalidValue)));
        }
        assert_eq!(owner.personal_generation().unwrap(), 1);
        drop(owner);
        drop(other);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generation_overflow_preserves_personal_data_on_failed_clear() {
        let mut state = UserState::in_memory().unwrap();
        state.set_bookmark(&key(), 123, "keep").unwrap();
        state
            .conn
            .execute(
                "INSERT INTO settings(key,value) VALUES (?1,?2)",
                params![PERSONAL_GENERATION_KEY, i64::MAX.to_string()],
            )
            .unwrap();
        assert!(matches!(
            state.clear_personal_reading_data(),
            Err(UserStateError::InvalidValue)
        ));
        assert_eq!(state.bookmark(&key()).unwrap(), Some((123, "keep".into())));
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
    fn disk_backed_handles_use_wal_and_wait_for_concurrent_writer() {
        use std::sync::mpsc;
        use std::thread;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-concurrency-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("user.sqlite");
        let first = UserState::open(&db).unwrap();
        let second = UserState::open(&db).unwrap();
        let mode: String = first
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");

        first.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        first.set_setting("pending", "write").unwrap();
        // A WAL reader sees the previous committed snapshot, not dirty data.
        assert_eq!(second.setting("pending").unwrap(), None);
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            sender
                .send(second.set_setting("concurrent", "ready").is_ok())
                .unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        first.conn.execute_batch("COMMIT").unwrap();
        assert!(receiver.recv_timeout(Duration::from_secs(5)).unwrap());
        worker.join().unwrap();
        assert_eq!(
            first.setting("concurrent").unwrap().as_deref(),
            Some("ready")
        );
        assert_eq!(first.setting("pending").unwrap().as_deref(), Some("write"));
        drop(first);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_schema_upgrade_rechecks_version_after_writer_lock() {
        use std::sync::mpsc;
        use std::thread;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-migration-lock-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("user.sqlite");
        let state = UserState::open(&db).unwrap();
        state.set_bookmark(&key(), 123, "retained").unwrap();
        state
            .conn
            .execute_batch(
                "DROP INDEX history_recent_idx;
                 DROP INDEX chat_threads_article_idx;
                 DROP INDEX chat_messages_thread_idx;
                 DROP INDEX pack_owners_digest_idx;
                 PRAGMA user_version=1;",
            )
            .unwrap();
        drop(state);

        // Simulate another process finishing the v1 -> v2 migration while
        // this process opens the same database. Version discovery must occur
        // AFTER the second opener acquires its own writer lock.
        let migrator = Connection::open(&db).unwrap();
        migrator
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE INDEX history_recent_idx ON history(visited_at DESC, id DESC);
                 CREATE INDEX chat_threads_article_idx
                   ON chat_threads(project, page_id, revision_id, id DESC);
                 CREATE INDEX chat_messages_thread_idx ON chat_messages(thread_id, id);
                 CREATE INDEX pack_owners_digest_idx ON pack_owners(digest, pack_id);
                 PRAGMA user_version=2;",
            )
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = UserState::open(&db).and_then(|state| state.bookmark(&key()));
            result_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(result_rx.recv_timeout(Duration::from_millis(100)).is_err());
        migrator.execute_batch("COMMIT").unwrap();
        assert_eq!(
            result_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap(),
            Some((123, "retained".into()))
        );
        worker.join().unwrap();
        drop(migrator);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parallel_initializers_publish_one_complete_schema() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-first-open-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("user.sqlite");
        let gate = Arc::new(Barrier::new(9));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let db = db.clone();
            let gate = Arc::clone(&gate);
            workers.push(thread::spawn(move || {
                gate.wait();
                let state = UserState::open(&db).unwrap();
                let version: i64 = state
                    .conn
                    .query_row("PRAGMA user_version", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(version, SCHEMA_VERSION);
                state.set_setting("migration.test", "ready").unwrap();
            }));
        }
        gate.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        let reopened = UserState::open(&db).unwrap();
        assert_eq!(
            reopened.setting("migration.test").unwrap().as_deref(),
            Some("ready")
        );
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
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
    fn backup_rejects_invalid_copied_rows_and_cleans_destination() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-backup-copy-audit-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("source.sqlite");
        let destination = root.join("backup.sqlite");
        let state = UserState::open(&db).unwrap();
        state.set_bookmark(&key(), 123, "preserved").unwrap();

        // A live connection can be corrupted after startup by a different
        // writer. The backup must audit the copied generation, not trust the
        // earlier open-time preflight or SQLite integrity_check alone.
        state
            .conn
            .execute(
                "INSERT INTO builder_jobs (job_id,status) VALUES ('forged','invalid')",
                [],
            )
            .unwrap();
        assert!(matches!(
            state.backup_to_path(&destination),
            Err(UserStateError::InvalidValue)
        ));
        assert!(!destination.exists(), "published an invalid backup");
        state
            .conn
            .execute("DELETE FROM builder_jobs WHERE job_id='forged'", [])
            .unwrap();
        state.backup_to_path(&destination).unwrap();
        let backup = UserState::open(&destination).unwrap();
        assert_eq!(
            backup.bookmark(&key()).unwrap(),
            Some((123, "preserved".into()))
        );
        drop(backup);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_backup_is_consistent_and_refuses_to_overwrite_existing_state() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-backup-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("user-backup.sqlite");
        let state = UserState::in_memory().unwrap();
        state.set_bookmark(&key(), 123, "saved").unwrap();
        state.set_reading_position(&key(), 123, 8192).unwrap();
        state.set_history_enabled(true).unwrap();
        state.record_visit(&key(), 123, 1_700_000_000).unwrap();
        state.add_pack_owner("physics", &"a".repeat(64)).unwrap();
        state.backup_to_path(&destination).unwrap();

        let backed_up = UserState::open(&destination).unwrap();
        assert_eq!(
            backed_up.bookmark(&key()).unwrap(),
            Some((123, "saved".into()))
        );
        assert_eq!(
            backed_up.reading_position(&key()).unwrap(),
            Some((123, 8192))
        );
        assert_eq!(backed_up.recent_history(10).unwrap().len(), 1);
        assert_eq!(backed_up.object_owner_count(&"a".repeat(64)).unwrap(), 1);
        drop(backed_up);

        let original_bytes = fs::read(&destination).unwrap();
        state.set_bookmark(&key(), 124, "newer").unwrap();
        assert!(matches!(
            state.backup_to_path(&destination),
            Err(UserStateError::Io(_))
        ));
        assert_eq!(fs::read(&destination).unwrap(), original_bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_restore_is_new_only_and_preserves_state_and_original() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-restore-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let backup = root.join("backup.sqlite");
        let restored_path = root.join("restored.sqlite");
        let original = UserState::in_memory().unwrap();
        original.set_bookmark(&key(), 123, "original").unwrap();
        original.set_reading_position(&key(), 123, 2048).unwrap();
        original.set_history_enabled(true).unwrap();
        original.record_visit(&key(), 123, 1_700_000_000).unwrap();
        original.add_pack_owner("physics", &"a".repeat(64)).unwrap();
        original.set_builder_job_status("build1", "queued").unwrap();
        original.backup_to_path(&backup).unwrap();
        let before = fs::read(&backup).unwrap();

        UserState::restore_backup_to_new_path(&backup, &restored_path).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), before);
        let restored = UserState::open(&restored_path).unwrap();
        assert_eq!(
            restored.bookmark(&key()).unwrap(),
            Some((123, "original".into()))
        );
        assert_eq!(
            restored.reading_position(&key()).unwrap(),
            Some((123, 2048))
        );
        assert_eq!(restored.recent_history(10).unwrap().len(), 1);
        assert_eq!(restored.object_owner_count(&"a".repeat(64)).unwrap(), 1);
        assert_eq!(
            restored.builder_job_status("build1").unwrap().as_deref(),
            Some("queued")
        );
        original.set_bookmark(&key(), 124, "newer").unwrap();
        assert_eq!(
            restored.bookmark(&key()).unwrap(),
            Some((123, "original".into()))
        );
        assert!(matches!(
            UserState::restore_backup_to_new_path(&backup, &restored_path),
            Err(UserStateError::Io(_))
        ));
        assert_eq!(fs::read(&backup).unwrap(), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&restored_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(restored);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_rejects_blank_future_and_incomplete_backup_without_publishing() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-restore-invalid-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let backup = root.join("untrusted.sqlite");
        let target = root.join("restored.sqlite");

        let empty = Connection::open(&backup).unwrap();
        drop(empty);
        assert!(matches!(
            UserState::restore_backup_to_new_path(&backup, &target),
            Err(UserStateError::UnsupportedSchema(0))
        ));
        assert!(!target.exists());

        let bad = Connection::open(&backup).unwrap();
        bad.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(bad);
        assert!(matches!(
            UserState::restore_backup_to_new_path(&backup, &target),
            Err(UserStateError::UnsupportedSchema(99))
        ));
        assert!(!target.exists());

        let bad = Connection::open(&backup).unwrap();
        bad.execute_batch("PRAGMA user_version=2;").unwrap();
        drop(bad);
        assert!(matches!(
            UserState::restore_backup_to_new_path(&backup, &target),
            Err(UserStateError::InvalidValue)
        ));
        assert!(!target.exists());

        fs::write(&backup, b"not sqlite").unwrap();
        assert!(UserState::restore_backup_to_new_path(&backup, &target).is_err());
        assert!(!target.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let real = root.join("real.sqlite");
            let state = UserState::open(&real).unwrap();
            drop(state);
            fs::remove_file(&backup).unwrap();
            symlink(&real, &backup).unwrap();
            assert!(matches!(
                UserState::restore_backup_to_new_path(&backup, &target),
                Err(UserStateError::InvalidValue)
            ));
            assert!(!target.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_of_v1_backup_migrates_after_open_without_losing_records() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-restore-v1-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let old_path = root.join("v1.sqlite");
        let backup = root.join("backup.sqlite");
        let restored_path = root.join("restored.sqlite");
        let old = UserState::open(&old_path).unwrap();
        old.set_bookmark(&key(), 321, "old schema").unwrap();
        old.add_pack_owner("physics", &"b".repeat(64)).unwrap();
        old.conn
            .execute_batch(
                "DROP INDEX history_recent_idx;
                 DROP INDEX chat_threads_article_idx;
                 DROP INDEX chat_messages_thread_idx;
                 DROP INDEX pack_owners_digest_idx;
                 PRAGMA user_version=1;",
            )
            .unwrap();
        old.backup_to_path(&backup).unwrap();
        UserState::restore_backup_to_new_path(&backup, &restored_path).unwrap();
        let restored = UserState::open(&restored_path).unwrap();
        assert_eq!(
            restored.bookmark(&key()).unwrap(),
            Some((321, "old schema".into()))
        );
        assert_eq!(restored.object_owner_count(&"b".repeat(64)).unwrap(), 1);
        let version: i64 = restored
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let index_count: i64 = restored
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='pack_owners_digest_idx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1);
        drop(restored);
        drop(old);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_spoofed_columns_triggers_and_missing_cascade_before_open_or_restore() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-schema-shape-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        for (name, tamper) in [
            (
                "renamed-column",
                "ALTER TABLE bookmarks RENAME COLUMN anchor TO forged;",
            ),
            (
                "injected-trigger",
                "CREATE TRIGGER injected AFTER INSERT ON settings BEGIN SELECT 1; END;",
            ),
            (
                "missing-cascade",
                "DROP TABLE chat_messages;
                 CREATE TABLE chat_messages (
                   id INTEGER PRIMARY KEY,
                   thread_id INTEGER NOT NULL,
                   role TEXT NOT NULL,
                   content TEXT NOT NULL
                 );
                 CREATE INDEX chat_messages_thread_idx ON chat_messages(thread_id, id);",
            ),
        ] {
            let source_path = root.join(format!("{name}.sqlite"));
            let destination = root.join(format!("{name}-restored.sqlite"));
            let state = UserState::open(&source_path).unwrap();
            state.conn.execute_batch(tamper).unwrap();
            drop(state);
            assert!(matches!(
                UserState::open(&source_path),
                Err(UserStateError::InvalidValue)
            ));
            assert!(matches!(
                UserState::restore_backup_to_new_path(&source_path, &destination),
                Err(UserStateError::InvalidValue)
            ));
            assert!(!destination.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_primary_key_nullability_and_index_spoofs_on_open_and_restore() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-schema-constraints-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        for (name, tamper) in [
            (
                "nullable-status",
                "DROP TABLE builder_jobs;
                 CREATE TABLE builder_jobs (job_id TEXT PRIMARY KEY, status TEXT);",
            ),
            (
                "wrong-primary-key",
                "DROP TABLE bookmarks;
                 CREATE TABLE bookmarks (
                   project TEXT NOT NULL,
                   page_id INTEGER NOT NULL,
                   revision_id INTEGER NOT NULL,
                   anchor TEXT NOT NULL,
                   PRIMARY KEY(project, revision_id)
                 );",
            ),
            (
                "wrong-index-column",
                "DROP INDEX history_recent_idx;
                 CREATE INDEX history_recent_idx ON history(project);",
            ),
            (
                "wrong-index-direction",
                "DROP INDEX history_recent_idx;
                 CREATE INDEX history_recent_idx ON history(visited_at ASC, id DESC);",
            ),
            (
                "unique-index",
                "DROP INDEX history_recent_idx;
                 CREATE UNIQUE INDEX history_recent_idx ON history(visited_at DESC, id DESC);",
            ),
            (
                "expression-index",
                "DROP INDEX history_recent_idx;
                 CREATE INDEX history_recent_idx ON history((visited_at + 1), id DESC);",
            ),
        ] {
            let source_path = root.join(format!("{name}.sqlite"));
            let destination = root.join(format!("{name}-restored.sqlite"));
            let state = UserState::open(&source_path).unwrap();
            state.conn.execute_batch(tamper).unwrap();
            drop(state);
            assert!(
                matches!(
                    UserState::open(&source_path),
                    Err(UserStateError::InvalidValue)
                ),
                "accepted spoofed schema on open: {name}"
            );
            assert!(
                matches!(
                    UserState::restore_backup_to_new_path(&source_path, &destination),
                    Err(UserStateError::InvalidValue)
                ),
                "accepted spoofed schema on restore: {name}"
            );
            assert!(!destination.exists(), "published invalid restore: {name}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_rejects_orphaned_chat_foreign_keys() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-restore-fk-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let backup = root.join("orphan.sqlite");
        let target = root.join("restored.sqlite");
        let source = UserState::open(&backup).unwrap();
        source
            .conn
            .execute_batch("PRAGMA foreign_keys=OFF;")
            .unwrap();
        source
            .conn
            .execute(
                "INSERT INTO chat_messages (thread_id,role,content)
                 VALUES (987654,'user','orphaned')",
                [],
            )
            .unwrap();
        drop(source);
        assert!(matches!(
            UserState::restore_backup_to_new_path(&backup, &target),
            Err(UserStateError::InvalidValue)
        ));
        assert!(!target.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reopen_rejects_current_schema_missing_required_table_or_index() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-schema-audit-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let missing_table = root.join("missing-table.sqlite");
        let state = UserState::open(&missing_table).unwrap();
        state
            .conn
            .execute_batch("DROP TABLE builder_jobs;")
            .unwrap();
        drop(state);
        assert!(matches!(
            UserState::open(&missing_table),
            Err(UserStateError::InvalidValue)
        ));

        let missing_index = root.join("missing-index.sqlite");
        let state = UserState::open(&missing_index).unwrap();
        state
            .conn
            .execute_batch("DROP INDEX history_recent_idx;")
            .unwrap();
        drop(state);
        assert!(matches!(
            UserState::open(&missing_index),
            Err(UserStateError::InvalidValue)
        ));
        let healthy = UserState::open(&root.join("healthy.sqlite")).unwrap();
        assert_eq!(healthy.setting("history.enabled").unwrap(), None);
        drop(healthy);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reopen_rejects_existing_check_and_foreign_key_corruption() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-corrupt-rows-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        for (name, tamper) in [
            (
                "violated-check",
                "PRAGMA ignore_check_constraints=ON;
                 INSERT INTO reading_positions
                   (project,page_id,revision_id,scroll_offset)
                   VALUES ('enwiki',1,1,-10);
                 PRAGMA ignore_check_constraints=OFF;",
            ),
            (
                "invalid-bookmark-page",
                "PRAGMA ignore_check_constraints=ON;
                 INSERT INTO bookmarks (project,page_id,revision_id,anchor)
                   VALUES ('enwiki',0,1,'anchor');
                 PRAGMA ignore_check_constraints=OFF;",
            ),
            (
                "invalid-bookmark-revision",
                "PRAGMA ignore_check_constraints=ON;
                 INSERT INTO bookmarks (project,page_id,revision_id,anchor)
                   VALUES ('enwiki',1,-1,'anchor');
                 PRAGMA ignore_check_constraints=OFF;",
            ),
            (
                "invalid-position-revision",
                "PRAGMA ignore_check_constraints=ON;
                 INSERT INTO reading_positions (project,page_id,revision_id,scroll_offset)
                   VALUES ('enwiki',1,0,10);
                 PRAGMA ignore_check_constraints=OFF;",
            ),
            (
                "orphaned-chat",
                "PRAGMA foreign_keys=OFF;
                 INSERT INTO chat_messages (thread_id,role,content)
                   VALUES (987654,'user','orphaned');",
            ),
        ] {
            let source = root.join(format!("{name}.sqlite"));
            let restored = root.join(format!("{name}-restored.sqlite"));
            let state = UserState::open(&source).unwrap();
            state.conn.execute_batch(tamper).unwrap();
            drop(state);
            assert!(
                matches!(UserState::open(&source), Err(UserStateError::InvalidValue)),
                "accepted corrupt user rows on reopen: {name}"
            );
            // SQLite may report CHECK violations as a query error rather
            // than a domain-level InvalidValue during restore preflight.
            assert!(
                UserState::restore_backup_to_new_path(&source, &restored).is_err(),
                "accepted corrupt user rows on restore: {name}"
            );
            assert!(!restored.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_state_and_restore_reject_invalid_rows_outside_sqlite_checks() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-semantic-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        for (name, tamper) in [
            (
                "history-timestamp",
                "INSERT INTO history (project,page_id,revision_id,visited_at)
                 VALUES ('enwiki',1,1,0);",
            ),
            (
                "history-project",
                "INSERT INTO history (project,page_id,revision_id,visited_at)
                 VALUES ('ENWIKI',1,1,100);",
            ),
            (
                "chat-revision",
                "INSERT INTO chat_threads (project,page_id,revision_id)
                 VALUES ('enwiki',1,0);",
            ),
            (
                "chat-role",
                "INSERT INTO chat_threads (project,page_id,revision_id)
                 VALUES ('enwiki',1,1);
                 INSERT INTO chat_messages (thread_id,role,content)
                 VALUES (1,'system','untrusted');",
            ),
            (
                "chat-content",
                "INSERT INTO chat_threads (project,page_id,revision_id)
                 VALUES ('enwiki',1,1);
                 INSERT INTO chat_messages (thread_id,role,content)
                 VALUES (1,'user','');",
            ),
            (
                "pack-digest",
                "INSERT INTO pack_owners (pack_id,digest) VALUES ('valid-pack','bad');",
            ),
            (
                "job-status",
                "INSERT INTO builder_jobs (job_id,status) VALUES ('job-1','unknown');",
            ),
            (
                "setting-key",
                "INSERT INTO settings (key,value) VALUES ('Bad Key','value');",
            ),
            (
                "bookmark-anchor",
                "INSERT INTO bookmarks (project,page_id,revision_id,anchor)
                 VALUES ('enwiki',1,1,replace(hex(zeroblob(2049)),'0','x'));",
            ),
            (
                "oversized-project",
                "INSERT INTO history (project,page_id,revision_id,visited_at)
                 VALUES (replace(hex(zeroblob(200)),'0','x'),1,1,100);",
            ),
            (
                "oversized-pack-id",
                "INSERT INTO pack_owners (pack_id,digest)
                 VALUES (replace(hex(zeroblob(100)),'0','x'),
                         'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');",
            ),
            (
                "oversized-setting",
                "INSERT INTO settings (key,value)
                 VALUES ('large',replace(hex(zeroblob(2049)),'0','x'));",
            ),
        ] {
            let source = root.join(format!("{name}.sqlite"));
            let destination = root.join(format!("{name}-restored.sqlite"));
            let state = UserState::open(&source).unwrap();
            state.conn.execute_batch(tamper).unwrap();
            drop(state);
            assert!(
                matches!(UserState::open(&source), Err(UserStateError::InvalidValue)),
                "accepted invalid existing row: {name}"
            );
            assert!(
                UserState::restore_backup_to_new_path(&source, &destination).is_err(),
                "accepted invalid backup row: {name}"
            );
            assert!(!destination.exists(), "published invalid backup: {name}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_rechecks_copied_rows_after_concurrent_source_mutation() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-user-state-restore-race-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let backup = root.join("source.sqlite");
        let destination = root.join("restored.sqlite");
        let state = UserState::open(&backup).unwrap();
        state.set_builder_job_status("good", "queued").unwrap();
        drop(state);

        // A separate SQLite writer commits after the source's successful
        // semantic preflight, but before VACUUM INTO copies the source.
        let result = UserState::restore_backup_with_preflight_hook(&backup, &destination, || {
            let writer = Connection::open(&backup).unwrap();
            writer
                .execute(
                    "INSERT INTO builder_jobs (job_id,status) VALUES (?1,?2)",
                    params!["forged", "invalid"],
                )
                .unwrap();
        });
        assert!(matches!(result, Err(UserStateError::InvalidValue)));
        assert!(!destination.exists(), "published a tampered backup");

        let writer = Connection::open(&backup).unwrap();
        writer
            .execute("DELETE FROM builder_jobs WHERE job_id='forged'", [])
            .unwrap();
        drop(writer);
        UserState::restore_backup_to_new_path(&backup, &destination).unwrap();
        let restored = UserState::open(&destination).unwrap();
        assert_eq!(
            restored.builder_job_status("good").unwrap().as_deref(),
            Some("queued")
        );
        drop(restored);
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
