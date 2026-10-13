//! Revision-keyed, bounded temporary HTML cache, separate from immutable packs.
//!
//! A cache hit is NOT an authoritative current revision or an offline-ready
//! article: mandatory media, source provenance and safe rendering must still
//! be checked by the reader. Corrupt cached bytes fail closed on every read.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use wiki_model::ArticleKey;

const SCHEMA_VERSION: i64 = 1;
const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;
const MAX_CACHE_BYTES: i64 = 64 * 1024 * 1024;
const MAX_CACHE_ROWS: i64 = 128;

#[derive(Debug)]
pub enum HtmlCacheError {
    Sql(rusqlite::Error),
    UnsupportedSchema(i64),
    InvalidIdentity,
    InvalidContent,
    DigestMismatch,
    RevisionConflict,
}

impl From<rusqlite::Error> for HtmlCacheError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedHtml {
    pub key: ArticleKey,
    pub revision_id: u64,
    pub title: String,
    pub html: String,
    pub fetched_at: i64,
}

pub struct RevisionHtmlCache {
    conn: Connection,
}

fn validate_key(key: &ArticleKey, revision_id: u64) -> Result<(i64, i64), HtmlCacheError> {
    key.validate()
        .map_err(|_| HtmlCacheError::InvalidIdentity)?;
    let page_id = i64::try_from(key.page_id).map_err(|_| HtmlCacheError::InvalidIdentity)?;
    let revision_id = i64::try_from(revision_id).map_err(|_| HtmlCacheError::InvalidIdentity)?;
    if page_id <= 0 || revision_id <= 0 {
        return Err(HtmlCacheError::InvalidIdentity);
    }
    Ok((page_id, revision_id))
}

fn valid_title(title: &str) -> bool {
    !title.is_empty()
        && title == title.trim()
        && title.len() <= 4096
        && !title.chars().any(char::is_control)
}

fn digest(html: &str) -> String {
    format!("{:x}", Sha256::digest(html.as_bytes()))
}

impl RevisionHtmlCache {
    pub fn open(path: &Path) -> Result<Self, HtmlCacheError> {
        Self::initialize(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, HtmlCacheError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut conn: Connection) -> Result<Self, HtmlCacheError> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if !(0..=SCHEMA_VERSION).contains(&version) {
            return Err(HtmlCacheError::UnsupportedSchema(version));
        }
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE cached_html (
                    project TEXT NOT NULL,
                    page_id INTEGER NOT NULL CHECK(page_id > 0),
                    revision_id INTEGER NOT NULL CHECK(revision_id > 0),
                    title TEXT NOT NULL,
                    html TEXT NOT NULL,
                    sha256 TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL CHECK(fetched_at > 0),
                    PRIMARY KEY(project, page_id, revision_id)
                 );
                 CREATE INDEX cached_html_age ON cached_html(fetched_at);",
            )?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    /// Store only already-sanitized, revision-pinned HTML. This API cannot
    /// authenticate its caller and never marks an article offline-complete.
    pub fn put(&mut self, article: &CachedHtml) -> Result<(), HtmlCacheError> {
        let (page_id, revision_id) = validate_key(&article.key, article.revision_id)?;
        if !valid_title(&article.title)
            || article.html.trim().is_empty()
            || article.html.len() > MAX_HTML_BYTES
            || article.fetched_at <= 0
        {
            return Err(HtmlCacheError::InvalidContent);
        }
        let tx = self.conn.transaction()?;
        // A revision key must never be silently rebound to different content.
        // An explicit remove is required before a replacement is accepted.
        let existing: Option<(String, String, String)> = tx
            .query_row(
                "SELECT title, html, sha256 FROM cached_html
                 WHERE project=?1 AND page_id=?2 AND revision_id=?3",
                params![article.key.project, page_id, revision_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((title, html, sha256)) = existing {
            if digest(&html) != sha256 {
                return Err(HtmlCacheError::DigestMismatch);
            }
            if title != article.title || html != article.html {
                return Err(HtmlCacheError::RevisionConflict);
            }
        }
        tx.execute(
            "INSERT INTO cached_html
                (project, page_id, revision_id, title, html, sha256, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(project, page_id, revision_id) DO UPDATE SET
                fetched_at=MAX(cached_html.fetched_at, excluded.fetched_at)",
            params![
                article.key.project,
                page_id,
                revision_id,
                article.title,
                article.html,
                digest(&article.html),
                article.fetched_at
            ],
        )?;
        // Bound both row count and aggregate stored HTML bytes. Eviction
        // never touches immutable snapshot content or user-state databases.
        loop {
            let (rows, bytes): (i64, i64) = tx.query_row(
                "SELECT count(*), coalesce(sum(length(CAST(html AS BLOB))), 0)
                 FROM cached_html",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if rows <= MAX_CACHE_ROWS && bytes <= MAX_CACHE_BYTES {
                break;
            }
            tx.execute(
                "DELETE FROM cached_html WHERE rowid = (
                    SELECT rowid FROM cached_html
                    ORDER BY fetched_at ASC, rowid ASC LIMIT 1
                 )",
                [],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Return only the exact requested revision, never an implicit "current"
    /// article. Digest validation catches stale or corrupted SQLite content.
    pub fn get(
        &self,
        key: &ArticleKey,
        revision_id: u64,
    ) -> Result<Option<CachedHtml>, HtmlCacheError> {
        let (page_id, revision) = validate_key(key, revision_id)?;
        let row: Option<(String, String, String, i64)> = self
            .conn
            .query_row(
                "SELECT title, html, sha256, fetched_at FROM cached_html
                 WHERE project=?1 AND page_id=?2 AND revision_id=?3",
                params![key.project, page_id, revision],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((title, html, sha256, fetched_at)) = row else {
            return Ok(None);
        };
        if !valid_title(&title)
            || html.trim().is_empty()
            || html.len() > MAX_HTML_BYTES
            || fetched_at <= 0
        {
            return Err(HtmlCacheError::InvalidContent);
        }
        if digest(&html) != sha256 {
            return Err(HtmlCacheError::DigestMismatch);
        }
        Ok(Some(CachedHtml {
            key: key.clone(),
            revision_id,
            title,
            html,
            fetched_at,
        }))
    }

    /// Evict every temporary HTML entry without touching immutable packs,
    /// explicitly saved offline content, or the separate user-state store.
    /// Returns the number of cache rows removed.
    pub fn clear(&mut self) -> Result<usize, HtmlCacheError> {
        Ok(self.conn.execute("DELETE FROM cached_html", [])?)
    }

    pub fn remove(&mut self, key: &ArticleKey, revision_id: u64) -> Result<(), HtmlCacheError> {
        let (page_id, revision_id) = validate_key(key, revision_id)?;
        self.conn.execute(
            "DELETE FROM cached_html WHERE project=?1 AND page_id=?2 AND revision_id=?3",
            params![key.project, page_id, revision_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(revision_id: u64) -> CachedHtml {
        CachedHtml {
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision_id,
            title: "Physics".into(),
            html: "<p>Physics</p>".into(),
            fetched_at: 100,
        }
    }

    #[test]
    fn exact_revision_roundtrip_and_deletion() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let first = article(123);
        cache.put(&first).unwrap();
        assert_eq!(cache.get(&first.key, 123).unwrap(), Some(first.clone()));
        assert_eq!(cache.get(&first.key, 124).unwrap(), None);
        cache.remove(&first.key, 123).unwrap();
        cache.remove(&first.key, 123).unwrap();
        assert_eq!(cache.get(&first.key, 123).unwrap(), None);
    }

    #[test]
    fn rejects_invalid_identity_content_and_corrupt_digest() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let mut bad = article(0);
        assert!(matches!(
            cache.put(&bad),
            Err(HtmlCacheError::InvalidIdentity)
        ));
        bad = article(123);
        bad.html.clear();
        assert!(matches!(
            cache.put(&bad),
            Err(HtmlCacheError::InvalidContent)
        ));
        bad = article(123);
        cache.put(&bad).unwrap();
        cache
            .conn
            .execute("UPDATE cached_html SET html='tampered'", [])
            .unwrap();
        assert!(matches!(
            cache.get(&bad.key, 123),
            Err(HtmlCacheError::DigestMismatch)
        ));
    }

    #[test]
    fn same_revision_is_immutable_but_identical_refresh_is_idempotent() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let original = article(123);
        cache.put(&original).unwrap();

        let mut refreshed = original.clone();
        refreshed.fetched_at = 200;
        cache.put(&refreshed).unwrap();
        assert_eq!(cache.get(&original.key, 123).unwrap(), Some(refreshed));

        let mut older = original.clone();
        older.fetched_at = 50;
        cache.put(&older).unwrap();
        assert_eq!(
            cache.get(&original.key, 123).unwrap().unwrap().fetched_at,
            200
        );

        let mut different_html = original.clone();
        different_html.html = "<p>Unrelated content</p>".into();
        assert!(matches!(
            cache.put(&different_html),
            Err(HtmlCacheError::RevisionConflict)
        ));
        let mut different_title = original.clone();
        different_title.title = "Unrelated title".into();
        assert!(matches!(
            cache.put(&different_title),
            Err(HtmlCacheError::RevisionConflict)
        ));
        assert_eq!(
            cache.get(&original.key, 123).unwrap().unwrap().html,
            original.html
        );

        // A new revision may legitimately carry different HTML.
        let mut next_revision = different_html;
        next_revision.revision_id = 124;
        cache.put(&next_revision).unwrap();
        assert_eq!(cache.get(&original.key, 124).unwrap(), Some(next_revision));
    }

    #[test]
    fn corrupt_existing_revision_cannot_be_overwritten_by_put() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let original = article(123);
        cache.put(&original).unwrap();
        cache
            .conn
            .execute("UPDATE cached_html SET html='<p>corrupted</p>'", [])
            .unwrap();
        assert!(matches!(
            cache.put(&original),
            Err(HtmlCacheError::DigestMismatch)
        ));
        assert!(matches!(
            cache.get(&original.key, 123),
            Err(HtmlCacheError::DigestMismatch)
        ));
    }

    #[test]
    fn clearing_temporary_cache_is_explicit_and_scoped() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let first = article(123);
        let mut second = article(124);
        second.key.project = "frwiki".into();
        cache.put(&first).unwrap();
        cache.put(&second).unwrap();

        assert_eq!(cache.clear().unwrap(), 2);
        assert_eq!(cache.get(&first.key, first.revision_id).unwrap(), None);
        assert_eq!(cache.get(&second.key, second.revision_id).unwrap(), None);
        assert_eq!(cache.clear().unwrap(), 0);
        cache.put(&first).unwrap();
        assert_eq!(cache.get(&first.key, first.revision_id).unwrap(), Some(first));
    }

    #[test]
    fn bounds_number_of_revision_rows_and_keeps_newest() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        for revision in 1..=140 {
            let mut item = article(revision);
            item.fetched_at = revision as i64;
            cache.put(&item).unwrap();
        }
        assert_eq!(cache.get(&article(1).key, 1).unwrap(), None);
        assert!(cache.get(&article(140).key, 140).unwrap().is_some());
        let rows: i64 = cache
            .conn
            .query_row("SELECT count(*) FROM cached_html", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, MAX_CACHE_ROWS);
    }
}
