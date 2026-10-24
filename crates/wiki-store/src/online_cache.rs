//! Revision-keyed, bounded temporary HTML cache, separate from immutable packs.
//!
//! A cache hit is NOT an authoritative current revision or an offline-ready
//! article: mandatory media, source provenance and safe rendering must still
//! be checked by the reader. Corrupt cached bytes fail closed on every read.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use wiki_model::ArticleKey;

const SCHEMA_VERSION: i64 = 2;
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
        && !title.chars().any(|ch| {
            ch.is_control()
                || matches!(
                    ch,
                    '\u{061c}'
                        | '\u{200e}'..='\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{200b}'
                        | '\u{2028}'..='\u{2029}'
                        | '\u{2060}'
                        | '\u{2066}'..='\u{206f}'
                        | '\u{feff}'
                )
        })
}

fn digest(html: &str) -> String {
    format!("{:x}", Sha256::digest(html.as_bytes()))
}

/// Bind all persisted metadata and HTML to the revision identity. Length
/// prefixes prevent ambiguous concatenations of attacker-controlled strings.
fn record_digest(article: &CachedHtml) -> String {
    let mut hash = Sha256::new();
    let page_id = article.key.page_id.to_be_bytes();
    let revision_id = article.revision_id.to_be_bytes();
    let fetched_at = article.fetched_at.to_be_bytes();
    for field in [
        article.key.project.as_bytes(),
        page_id.as_slice(),
        revision_id.as_slice(),
        article.title.as_bytes(),
        article.html.as_bytes(),
        fetched_at.as_slice(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    format!("{:x}", hash.finalize())
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
        if version < SCHEMA_VERSION {
            let tx = conn.transaction()?;
            if version == 1 {
                // This is a disposable *temporary* cache, not saved articles.
                // V1 did not authenticate title/identity/fetch timestamp.
                // Never bless legacy metadata by rehashing it in place.
                tx.execute_batch("DROP TABLE cached_html;")?;
            }
            tx.execute_batch(
                "CREATE TABLE cached_html (
                    project TEXT NOT NULL,
                    page_id INTEGER NOT NULL CHECK(page_id > 0),
                    revision_id INTEGER NOT NULL CHECK(revision_id > 0),
                    title TEXT NOT NULL,
                    html TEXT NOT NULL,
                    sha256 TEXT NOT NULL,
                    record_sha256 TEXT NOT NULL,
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
        let existing: Option<(String, String, String, String, i64)> = tx
            .query_row(
                "SELECT title, html, sha256, record_sha256, fetched_at
                 FROM cached_html
                 WHERE project=?1 AND page_id=?2 AND revision_id=?3",
                params![article.key.project, page_id, revision_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let mut stored = article.clone();
        if let Some((title, html, sha256, record_sha256, fetched_at)) = existing {
            let previous = CachedHtml {
                key: article.key.clone(),
                revision_id: article.revision_id,
                title,
                html,
                fetched_at,
            };
            if digest(&previous.html) != sha256 || record_digest(&previous) != record_sha256 {
                return Err(HtmlCacheError::DigestMismatch);
            }
            if previous.title != article.title || previous.html != article.html {
                return Err(HtmlCacheError::RevisionConflict);
            }
            stored.fetched_at = stored.fetched_at.max(previous.fetched_at);
        }
        tx.execute(
            "INSERT INTO cached_html
                (project, page_id, revision_id, title, html, sha256, record_sha256, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(project, page_id, revision_id) DO UPDATE SET
                fetched_at=excluded.fetched_at,
                record_sha256=excluded.record_sha256",
            params![
                article.key.project,
                page_id,
                revision_id,
                article.title,
                article.html,
                digest(&article.html),
                record_digest(&stored),
                stored.fetched_at
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
        let row: Option<(String, String, String, String, i64)> = self
            .conn
            .query_row(
                "SELECT title, html, sha256, record_sha256, fetched_at
                 FROM cached_html
                 WHERE project=?1 AND page_id=?2 AND revision_id=?3",
                params![key.project, page_id, revision],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, html, sha256, record_sha256, fetched_at)) = row else {
            return Ok(None);
        };
        if !valid_title(&title)
            || html.trim().is_empty()
            || html.len() > MAX_HTML_BYTES
            || fetched_at <= 0
        {
            return Err(HtmlCacheError::InvalidContent);
        }
        let article = CachedHtml {
            key: key.clone(),
            revision_id,
            title,
            html,
            fetched_at,
        };
        if digest(&article.html) != sha256 || record_digest(&article) != record_sha256 {
            return Err(HtmlCacheError::DigestMismatch);
        }
        Ok(Some(article))
    }

    /// Enumerate verified temporary copies, newest fetch first.
    /// This is not evidence of current upstream content or offline media.
    pub fn list_cached_revisions(
        &self,
        key: &ArticleKey,
    ) -> Result<Vec<CachedHtml>, HtmlCacheError> {
        let (page_id, _) = validate_key(key, 1)?;
        let mut statement = self.conn.prepare(
            "SELECT revision_id FROM cached_html
             WHERE project=?1 AND page_id=?2
             ORDER BY fetched_at DESC, revision_id DESC",
        )?;
        let ids = statement
            .query_map(params![key.project, page_id], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut results = Vec::with_capacity(ids.len());
        for id in ids {
            let revision_id = u64::try_from(id).map_err(|_| HtmlCacheError::InvalidIdentity)?;
            let item = self
                .get(key, revision_id)?
                .ok_or(HtmlCacheError::InvalidContent)?;
            results.push(item);
        }
        Ok(results)
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
    fn cached_title_rejects_directional_spoofing_without_disallowing_rtl_text() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        for (project, title) in [("arwiki", "الفيزياء"), ("hewiki", "פיזיקה")] {
            let mut item = article(123);
            item.key.project = project.into();
            item.title = title.into();
            cache.put(&item).unwrap();
            assert_eq!(cache.get(&item.key, 123).unwrap(), Some(item));
        }
        for marker in [
            '\u{061c}', '\u{200b}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202e}',
            '\u{2028}', '\u{2029}', '\u{2060}', '\u{2066}', '\u{2069}', '\u{206a}',
            '\u{206f}', '\u{feff}',
        ] {
            let mut item = article(125);
            item.title = format!("Physics{marker}Spoof");
            assert!(matches!(
                cache.put(&item),
                Err(HtmlCacheError::InvalidContent)
            ));
            assert_eq!(cache.get(&item.key, 125).unwrap(), None);
        }
        let normal = article(130);
        cache.put(&normal).unwrap();
        cache
            .conn
            .execute(
                "UPDATE cached_html SET title=?1 WHERE project='enwiki' AND revision_id=130",
                params!["Physics\u{202e}Spoof"],
            )
            .unwrap();
        assert!(matches!(
            cache.get(&normal.key, 130),
            Err(HtmlCacheError::InvalidContent)
        ));
    }

    #[test]
    fn rejects_invisible_cache_title_controls_on_read_and_preserves_joiners() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        // ZWNJ and ZWJ are meaningful in Persian/Indic text, not spoof markers.
        let mut natural = article(126);
        natural.key.project = "fawiki".into();
        natural.title = "می\u{200c}روم".into();
        cache.put(&natural).unwrap();
        assert_eq!(cache.get(&natural.key, 126).unwrap(), Some(natural));
        let mut joined = article(127);
        joined.key.project = "hiwiki".into();
        joined.title = "क्\u{200d}ष".into();
        cache.put(&joined).unwrap();
        assert_eq!(cache.get(&joined.key, 127).unwrap(), Some(joined));

        let original = article(128);
        cache.put(&original).unwrap();
        for marker in ['\u{200b}', '\u{2028}', '\u{2029}', '\u{2060}', '\u{206a}', '\u{206f}', '\u{feff}'] {
            cache.conn.execute(
                "UPDATE cached_html SET title=?1 WHERE project='enwiki' AND revision_id=128",
                params![format!("Physics{marker}Spoof")],
            ).unwrap();
            assert!(matches!(
                cache.get(&original.key, 128),
                Err(HtmlCacheError::InvalidContent)
            ));
        }
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
        assert_eq!(
            cache.get(&first.key, first.revision_id).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn detects_title_timestamp_and_identity_tampering() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let original = article(123);
        for (column, value) in [
            ("title", "'Forged title'"),
            ("fetched_at", "999999"),
            ("revision_id", "124"),
            ("page_id", "43"),
            ("project", "'frwiki'"),
        ] {
            cache.clear().unwrap();
            cache.put(&original).unwrap();
            cache
                .conn
                .execute(&format!("UPDATE cached_html SET {column}={value}"), [])
                .unwrap();
            let mut requested = original.clone();
            if column == "revision_id" {
                requested.revision_id = 124;
            } else if column == "page_id" {
                requested.key.page_id = 43;
            } else if column == "project" {
                requested.key.project = "frwiki".into();
            }
            assert!(matches!(
                cache.get(&requested.key, requested.revision_id),
                Err(HtmlCacheError::DigestMismatch)
            ));
        }
    }

    #[test]
    fn legacy_v1_cache_is_discarded_not_trusted_as_new_metadata() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cached_html (
                project TEXT NOT NULL, page_id INTEGER NOT NULL,
                revision_id INTEGER NOT NULL, title TEXT NOT NULL,
                html TEXT NOT NULL, sha256 TEXT NOT NULL,
                fetched_at INTEGER NOT NULL,
                PRIMARY KEY(project, page_id, revision_id)
             );
             CREATE INDEX cached_html_age ON cached_html(fetched_at);
             INSERT INTO cached_html VALUES (
                'enwiki', 42, 123, 'Forged title', '<p>Physics</p>', 'legacy', 100
             );
             PRAGMA user_version=1;",
        )
        .unwrap();
        let cache = RevisionHtmlCache::initialize(conn).unwrap();
        assert_eq!(cache.get(&article(123).key, 123).unwrap(), None);
        let version: i64 = cache
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn lists_only_verified_temporary_revisions_for_one_page() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let mut older = article(10);
        older.fetched_at = 100;
        let mut latest = article(11);
        latest.fetched_at = 300;
        let mut middle = article(12);
        middle.fetched_at = 200;
        cache.put(&older).unwrap();
        cache.put(&latest).unwrap();
        cache.put(&middle).unwrap();
        let mut other = article(13);
        other.key.project = "frwiki".into();
        cache.put(&other).unwrap();
        assert_eq!(
            cache.list_cached_revisions(&older.key).unwrap(),
            vec![latest.clone(), middle, older.clone()]
        );
        assert_eq!(
            cache.list_cached_revisions(&other.key).unwrap(),
            vec![other]
        );
        cache
            .conn
            .execute(
                "UPDATE cached_html SET title='Changed' WHERE revision_id=11",
                [],
            )
            .unwrap();
        assert!(matches!(
            cache.list_cached_revisions(&older.key),
            Err(HtmlCacheError::DigestMismatch)
        ));
        cache.clear().unwrap();
        assert!(cache.list_cached_revisions(&older.key).unwrap().is_empty());
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
