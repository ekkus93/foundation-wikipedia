//! SQLite index for one immutable snapshot's independently compressed records.
//!
//! A catalog is not a source-authenticity proof. Activation must independently
//! verify the full upstream manifest and every required object before use.
//! Each lookup rechecks the exact frame digest and record identity.

use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use wiki_model::{wikipedia_title_key, Article, ArticleKey, ModelError, PageRecord};

use crate::record_codec::{decode_record_at, RecordCodecError, MAX_COMPRESSED_RECORD_BYTES};

const MAX_FRAME_BYTES: u64 = MAX_COMPRESSED_RECORD_BYTES + 40;
const MAX_TITLE_PREFIX_SCAN: usize = 10_000;

#[derive(Debug)]
pub enum CatalogError {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    Codec(RecordCodecError),
    Model(ModelError),
    UnsafeShardName,
    InvalidFrame,
    InvalidCatalogEntry,
    DigestMismatch,
    IdentityMismatch,
    DanglingRedirect,
    RedirectCycle,
    RedirectTooDeep,
}

impl From<rusqlite::Error> for CatalogError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}
impl From<std::io::Error> for CatalogError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<RecordCodecError> for CatalogError {
    fn from(value: RecordCodecError) -> Self {
        Self::Codec(value)
    }
}
impl From<ModelError> for CatalogError {
    fn from(value: ModelError) -> Self {
        Self::Model(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub key: ArticleKey,
    pub title: String,
    pub revision_id: Option<u64>,
    pub wikidata_id: Option<String>,
    pub shard_name: String,
    pub offset: u64,
    pub frame_bytes: u64,
    pub frame_sha256: String,
}

type CatalogRow = (
    String,
    Option<i64>,
    Option<String>,
    String,
    i64,
    i64,
    String,
);

pub struct SnapshotCatalog {
    conn: Connection,
}

fn valid_shard_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !reserved
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// Match MediaWiki's first-letter title behavior for snapshot navigation.
/// Underscores are spaces; the rest of the title remains case-sensitive.
/// Avoid guessing Unicode characters with multi-scalar uppercase expansions.
fn title_key(title: &str) -> Result<String, CatalogError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(CatalogError::InvalidCatalogEntry);
    }
    Ok(wikipedia_title_key(trimmed))
}

fn as_i64(value: u64) -> Result<i64, CatalogError> {
    i64::try_from(value).map_err(|_| CatalogError::InvalidCatalogEntry)
}
fn as_u64(value: i64) -> Result<u64, CatalogError> {
    u64::try_from(value).map_err(|_| CatalogError::InvalidCatalogEntry)
}

fn validate_snapshot_root(root: &Path) -> Result<(), CatalogError> {
    // Reject symlinks in every existing path component, not only the final
    // snapshot directory. Otherwise a regular root reached through a linked
    // parent can silently read records from an unrelated directory.
    // This remains a preflight: callers must prevent concurrent renames.
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(ancestor)?;
        if meta.file_type().is_symlink() {
            return Err(CatalogError::InvalidFrame);
        }
    }
    let meta = fs::symlink_metadata(root)?;
    if !meta.is_dir() {
        return Err(CatalogError::InvalidFrame);
    }
    Ok(())
}

fn read_verified_frame(
    root: &Path,
    shard_name: &str,
    offset: u64,
    frame_bytes: u64,
    expected_digest: Option<&str>,
) -> Result<(PageRecord, String), CatalogError> {
    if !valid_shard_name(shard_name) {
        return Err(CatalogError::UnsafeShardName);
    }
    validate_snapshot_root(root)?;
    if !(40..=MAX_FRAME_BYTES).contains(&frame_bytes) {
        return Err(CatalogError::InvalidFrame);
    }
    let path = root.join(shard_name);
    let prior = fs::symlink_metadata(&path)?;
    if !prior.is_file() || prior.file_type().is_symlink() {
        return Err(CatalogError::InvalidFrame);
    }
    let mut file = File::open(&path)?;
    let opened = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if prior.ino() != opened.ino() || prior.dev() != opened.dev() || opened.nlink() != 1 {
            return Err(CatalogError::InvalidFrame);
        }
    }
    if offset
        .checked_add(frame_bytes)
        .is_none_or(|end| end > opened.len())
    {
        return Err(CatalogError::InvalidFrame);
    }
    let length = usize::try_from(frame_bytes).map_err(|_| CatalogError::InvalidFrame)?;
    let mut frame = vec![0; length];
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(&mut frame)?;
    let after = file.metadata()?;
    if after.len() != opened.len() || !after.is_file() {
        return Err(CatalogError::InvalidFrame);
    }
    let digest = format!("{:x}", Sha256::digest(&frame));
    if expected_digest.is_some_and(|expected| expected != digest) {
        return Err(CatalogError::DigestMismatch);
    }
    Ok((decode_record_at(&frame, 0)?.record, digest))
}

fn verify_shard_length(root: &Path, name: &str, expected: u64) -> Result<(), CatalogError> {
    validate_snapshot_root(root)?;
    let metadata = fs::symlink_metadata(root.join(name))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != expected {
        return Err(CatalogError::InvalidFrame);
    }
    Ok(())
}

fn identity(record: &PageRecord) -> (ArticleKey, String, Option<u64>, Option<String>, Vec<String>) {
    match record {
        PageRecord::Article(article) => (
            article.key.clone(),
            article.title.clone(),
            Some(article.revision.revision_id),
            article.wikidata_id.clone(),
            article.aliases.clone(),
        ),
        PageRecord::Redirect(redirect) => (
            redirect.from.clone(),
            redirect.title.clone(),
            None,
            None,
            Vec::new(),
        ),
    }
}

fn validate_catalog_path(path: &Path) -> Result<(), CatalogError> {
    let parent = path
        .parent()
        .filter(|part| !part.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    validate_snapshot_root(parent)?;
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(CatalogError::InvalidCatalogEntry);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() != 1 {
                    return Err(CatalogError::InvalidCatalogEntry);
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CatalogError::Io(error)),
    }
}

impl SnapshotCatalog {
    pub fn open(path: &Path) -> Result<Self, CatalogError> {
        validate_catalog_path(path)?;
        Self::initialize(Connection::open(path)?)
    }

    /// Open an existing activated snapshot catalog for reader queries only.
    /// Never create a missing database, migrate schemas or permit mutation.
    pub fn open_read_only(path: &Path) -> Result<Self, CatalogError> {
        validate_catalog_path(path)?;
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
        for table in ["records", "title_index"] {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |row| row.get(0),
            )?;
            if count != 1 {
                return Err(CatalogError::InvalidCatalogEntry);
            }
        }
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self, CatalogError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(conn: Connection) -> Result<Self, CatalogError> {
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS records (
               project TEXT NOT NULL,
               page_id INTEGER NOT NULL,
               title TEXT NOT NULL,
               revision_id INTEGER,
               wikidata_id TEXT,
               shard_name TEXT NOT NULL,
               frame_offset INTEGER NOT NULL,
               frame_bytes INTEGER NOT NULL,
               frame_sha256 TEXT NOT NULL,
               PRIMARY KEY(project, page_id)
             );
             CREATE TABLE IF NOT EXISTS title_index (
               project TEXT NOT NULL,
               normalized_title TEXT NOT NULL,
               page_id INTEGER NOT NULL,
               PRIMARY KEY(project, normalized_title),
               FOREIGN KEY(project, page_id) REFERENCES records(project, page_id)
             );
             CREATE INDEX IF NOT EXISTS records_wikidata
               ON records(project, wikidata_id);
             CREATE INDEX IF NOT EXISTS title_index_by_page
               ON title_index(project, page_id);",
        )?;
        Ok(Self { conn })
    }

    /// Only the decoded bytes from a bounded shard frame decide catalog identity.
    /// All title inserts and the record entry are one SQLite transaction.
    pub fn insert_verified(
        &mut self,
        root: &Path,
        shard_name: &str,
        offset: u64,
        frame_bytes: u64,
    ) -> Result<ArticleKey, CatalogError> {
        let (record, digest) = read_verified_frame(root, shard_name, offset, frame_bytes, None)?;
        record.validate()?;
        let (key, title, revision, wikidata, aliases) = identity(&record);
        let page_id = as_i64(key.page_id)?;
        let revision_id = revision.map(as_i64).transpose()?;
        let offset = as_i64(offset)?;
        let frame_bytes = as_i64(frame_bytes)?;
        let mut names = BTreeSet::new();
        for name in std::iter::once(&title).chain(aliases.iter()) {
            names.insert(title_key(name)?);
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO records
               (project,page_id,title,revision_id,wikidata_id,shard_name,frame_offset,frame_bytes,frame_sha256)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![key.project, page_id, title, revision_id, wikidata, shard_name, offset, frame_bytes, digest],
        )?;
        for name in names {
            tx.execute(
                "INSERT INTO title_index (project, normalized_title, page_id) VALUES (?1,?2,?3)",
                params![key.project, name, page_id],
            )?;
        }
        tx.commit()?;
        Ok(key)
    }

    pub fn lookup_key(
        &self,
        project: &str,
        title: &str,
    ) -> Result<Option<ArticleKey>, CatalogError> {
        let title = title_key(title)?;
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT page_id FROM title_index WHERE project=?1 AND normalized_title=?2",
                params![project, title],
                |row| row.get(0),
            )
            .optional()?;
        found
            .map(|page_id| {
                Ok(ArticleKey {
                    project: project.to_owned(),
                    page_id: as_u64(page_id)?,
                })
            })
            .transpose()
    }

    /// Prefix-search already indexed snapshot titles, aliases and redirects.
    /// Results are exact decoded articles with frame digests verified through
    /// resolve_title. Snapshot trust and required media are separate gates.
    /// A bounded scan fails instead of silently returning a misleading subset
    /// if one prefix maps to too many aliases of a small set of pages.
    pub fn search_title_prefix(
        &self,
        root: &Path,
        project: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<Article>, CatalogError> {
        ArticleKey {
            project: project.to_owned(),
            page_id: 1,
        }
        .validate()?;
        if !(1..=100).contains(&limit)
            || prefix.is_empty()
            || prefix != prefix.trim()
            || prefix.len() > 512
            || prefix.chars().any(|ch| {
                ch.is_control()
                    || matches!(
                        ch,
                        '\u{061c}'
                            | '\u{200e}'..='\u{200f}'
                            | '\u{202a}'..='\u{202e}'
                            | '\u{2066}'..='\u{2069}'
                    )
            })
        {
            return Err(CatalogError::InvalidCatalogEntry);
        }
        let normalized = title_key(prefix)?;
        let mut stmt = self.conn.prepare(
            "SELECT normalized_title FROM title_index
             WHERE project=?1 AND normalized_title>=?2
             ORDER BY normalized_title",
        )?;
        let rows = stmt.query_map(params![project, normalized], |row| row.get::<_, String>(0))?;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for (scanned, row) in rows.enumerate() {
            let title = row?;
            if !title.starts_with(&normalized) {
                break;
            }
            if scanned >= MAX_TITLE_PREFIX_SCAN {
                return Err(CatalogError::InvalidCatalogEntry);
            }
            let (key, record) = self
                .resolve_title(root, project, &title)?
                .ok_or(CatalogError::InvalidCatalogEntry)?;
            if seen.insert(key.page_id) {
                match record {
                    PageRecord::Article(article) => result.push(*article),
                    PageRecord::Redirect(_) => return Err(CatalogError::DanglingRedirect),
                }
            }
            if result.len() == limit {
                break;
            }
        }
        Ok(result)
    }

    pub fn lookup_wikidata(
        &self,
        project: &str,
        item: &str,
    ) -> Result<Vec<ArticleKey>, CatalogError> {
        let mut stmt = self.conn.prepare(
            "SELECT page_id FROM records WHERE project=?1 AND wikidata_id=?2 ORDER BY page_id",
        )?;
        let mut matches = Vec::new();
        for row in stmt.query_map(params![project, item], |row| row.get::<_, i64>(0))? {
            matches.push(ArticleKey {
                project: project.to_owned(),
                page_id: as_u64(row?)?,
            });
        }
        Ok(matches)
    }

    /// Resolve bounded Wikidata matches from digest-verified immutable records.
    /// Source authenticity and required media remain separate caller checks.
    pub fn lookup_wikidata_verified(
        &self,
        root: &Path,
        project: &str,
        item: &str,
        limit: usize,
    ) -> Result<Vec<Article>, CatalogError> {
        ArticleKey {
            project: project.to_owned(),
            page_id: 1,
        }
        .validate()?;
        let digits = item.strip_prefix('Q').unwrap_or("");
        if !(1..=100).contains(&limit)
            || item.len() > 64
            || digits.is_empty()
            || digits.starts_with('0')
            || !digits.bytes().all(|digit| digit.is_ascii_digit())
        {
            return Err(CatalogError::InvalidCatalogEntry);
        }
        let mut stmt = self.conn.prepare(
            "SELECT page_id FROM records WHERE project=?1 AND wikidata_id=?2
             ORDER BY page_id LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![project, item, limit as i64], |row| {
            row.get::<_, i64>(0)
        })?;
        let mut articles = Vec::new();
        for row in rows {
            let key = ArticleKey {
                project: project.to_owned(),
                page_id: as_u64(row?)?,
            };
            match self.read_record(root, &key)? {
                Some(PageRecord::Article(article))
                    if article.wikidata_id.as_deref() == Some(item) =>
                {
                    articles.push(*article);
                }
                _ => return Err(CatalogError::InvalidCatalogEntry),
            }
        }
        Ok(articles)
    }

    pub fn lookup_entry(&self, key: &ArticleKey) -> Result<Option<CatalogEntry>, CatalogError> {
        key.validate()?;
        let row: Option<CatalogRow> =
            self.conn.query_row(
                "SELECT title,revision_id,wikidata_id,shard_name,frame_offset,frame_bytes,frame_sha256
                   FROM records WHERE project=?1 AND page_id=?2",
                params![key.project, as_i64(key.page_id)?],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))
            ).optional()?;
        row.map(
            |(title, revision_id, wikidata_id, shard_name, offset, frame_bytes, frame_sha256)| {
                if !valid_shard_name(&shard_name)
                    || frame_sha256.len() != 64
                    || !frame_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(CatalogError::InvalidCatalogEntry);
                }
                Ok(CatalogEntry {
                    key: key.clone(),
                    title,
                    revision_id: revision_id.map(as_u64).transpose()?,
                    wikidata_id,
                    shard_name,
                    offset: as_u64(offset)?,
                    frame_bytes: as_u64(frame_bytes)?,
                    frame_sha256,
                })
            },
        )
        .transpose()
    }

    /// Audit every indexed frame before publishing a snapshot. Each referenced
    /// shard must consist entirely of contiguous, digest-verified, decoded
    /// records: missing frames, gaps, overlaps and trailing bytes fail closed.
    /// This verifies catalog coverage, not source authenticity or media assets.
    pub fn verify_shard_coverage(&self, root: &Path) -> Result<usize, CatalogError> {
        let mut stmt = self.conn.prepare(
            "SELECT project, page_id, shard_name, frame_offset, frame_bytes
             FROM records ORDER BY shard_name, frame_offset",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut current_shard: Option<String> = None;
        let mut next_offset = 0u64;
        let mut verified = 0usize;
        for row in rows {
            let (project, page_id, shard, offset, frame_bytes) = row?;
            if !valid_shard_name(&shard) {
                return Err(CatalogError::UnsafeShardName);
            }
            if current_shard.as_deref() != Some(&shard) {
                if let Some(previous) = &current_shard {
                    verify_shard_length(root, previous, next_offset)?;
                }
                current_shard = Some(shard);
                next_offset = 0;
            }
            let offset = as_u64(offset)?;
            let frame_bytes = as_u64(frame_bytes)?;
            if offset != next_offset || !(40..=MAX_FRAME_BYTES).contains(&frame_bytes) {
                return Err(CatalogError::InvalidFrame);
            }
            let key = ArticleKey {
                project,
                page_id: as_u64(page_id)?,
            };
            // Rehash and decode the exact frame, including the catalog's
            // revision/title/Wikidata identity checks.
            let record = self
                .read_record(root, &key)?
                .ok_or(CatalogError::InvalidCatalogEntry)?;
            // The title index is derived state. Audit every canonical title
            // and alias against the authenticated record, including omissions
            // and injected aliases, before activating a snapshot.
            let (_, title, _, _, aliases) = identity(&record);
            let expected_titles: BTreeSet<String> = std::iter::once(title.as_str())
                .chain(aliases.iter().map(String::as_str))
                .map(title_key)
                .collect::<Result<_, _>>()?;
            let mut title_stmt = self.conn.prepare(
                "SELECT normalized_title FROM title_index WHERE project=?1 AND page_id=?2",
            )?;
            let actual_titles: BTreeSet<String> = title_stmt
                .query_map(params![&key.project, as_i64(key.page_id)?], |row| {
                    row.get(0)
                })?
                .collect::<Result<_, _>>()?;
            if actual_titles != expected_titles {
                return Err(CatalogError::InvalidCatalogEntry);
            }
            next_offset = offset
                .checked_add(frame_bytes)
                .ok_or(CatalogError::InvalidFrame)?;
            verified += 1;
        }
        let last = current_shard.ok_or(CatalogError::InvalidCatalogEntry)?;
        verify_shard_length(root, &last, next_offset)?;
        // A corrupted SQLite file could also carry orphaned index rows that
        // the per-record audit above would never encounter.
        let orphaned: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM title_index AS t LEFT JOIN records AS r
             ON r.project=t.project AND r.page_id=t.page_id
             WHERE r.page_id IS NULL LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if orphaned.is_some() {
            return Err(CatalogError::InvalidCatalogEntry);
        }
        Ok(verified)
    }

    /// Require the indexed shard inventory to match the verified manifest's
    /// expected shard names. The caller must authenticate the manifest first.
    pub fn verify_expected_shards(
        &self,
        root: &Path,
        expected: &BTreeSet<String>,
    ) -> Result<usize, CatalogError> {
        let count = self.verify_shard_coverage(root)?;
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT shard_name FROM records")?;
        let indexed: BTreeSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        if &indexed != expected {
            return Err(CatalogError::InvalidCatalogEntry);
        }
        Ok(count)
    }

    /// Cross-check complete shard bytes against a previously authenticated
    /// manifest using bounded streaming reads, not whole-shard allocation.
    pub fn verify_manifest_shard_hashes(
        &self,
        root: &Path,
        expected: &std::collections::BTreeMap<String, (u64, String)>,
    ) -> Result<usize, CatalogError> {
        let names = expected.keys().cloned().collect();
        let count = self.verify_expected_shards(root, &names)?;
        for (name, (bytes, sha256)) in expected {
            if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(CatalogError::InvalidCatalogEntry);
            }
            let path = root.join(name);
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != *bytes
            {
                return Err(CatalogError::InvalidFrame);
            }
            let mut file = File::open(path)?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 65536];
            let mut read_bytes = 0u64;
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                read_bytes = read_bytes
                    .checked_add(n as u64)
                    .ok_or(CatalogError::InvalidFrame)?;
                if read_bytes > *bytes {
                    return Err(CatalogError::InvalidFrame);
                }
                hasher.update(&buffer[..n]);
            }
            if read_bytes != *bytes || file.metadata()?.len() != *bytes {
                return Err(CatalogError::InvalidFrame);
            }
            if format!("{:x}", hasher.finalize()) != *sha256 {
                return Err(CatalogError::DigestMismatch);
            }
        }
        Ok(count)
    }

    /// Visit catalog entries in stable key order, decoding and verifying one
    /// frame at a time. No complete corpus is materialized in memory. The
    /// caller must authenticate the source snapshot before using this index.
    pub fn visit_records<E, F>(&self, root: &Path, mut visit: F) -> Result<(), E>
    where
        E: From<CatalogError>,
        F: FnMut(PageRecord) -> Result<(), E>,
    {
        let mut stmt = self
            .conn
            .prepare("SELECT project, page_id FROM records ORDER BY project, page_id")
            .map_err(CatalogError::from)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(CatalogError::from)?;
        for row in rows {
            let (project, page_id) = row.map_err(CatalogError::from)?;
            let key = ArticleKey {
                project,
                page_id: as_u64(page_id)?,
            };
            let record = self
                .read_record(root, &key)?
                .ok_or(CatalogError::InvalidCatalogEntry)?;
            visit(record)?;
        }
        Ok(())
    }

    /// Resolve an article title through at most 32 verified redirect records.
    /// A dangling target, cycle or excessive chain never counts as success.
    pub fn resolve_title(
        &self,
        root: &Path,
        project: &str,
        title: &str,
    ) -> Result<Option<(ArticleKey, PageRecord)>, CatalogError> {
        let requested_title = title_key(title)?;
        let Some(mut key) = self.lookup_key(project, title)? else {
            return Ok(None);
        };
        let mut visited = HashSet::new();
        for _ in 0..32 {
            if !visited.insert(key.clone()) {
                return Err(CatalogError::RedirectCycle);
            }
            let Some(record) = self.read_record(root, &key)? else {
                return Err(CatalogError::DanglingRedirect);
            };
            if visited.len() == 1 {
                // The title index is derived SQLite state, not a verified
                // source of article identity. Reject a forged index mapping
                // even when it points at an otherwise valid immutable frame.
                let (_, canonical, _, _, aliases) = identity(&record);
                let matches_indexed_title = std::iter::once(canonical.as_str())
                    .chain(aliases.iter().map(String::as_str))
                    .any(|name| title_key(name).is_ok_and(|value| value == requested_title));
                if !matches_indexed_title {
                    return Err(CatalogError::InvalidCatalogEntry);
                }
            }
            match record {
                PageRecord::Redirect(redirect) => key = redirect.to,
                article @ PageRecord::Article(_) => return Ok(Some((key, article))),
            }
        }
        Err(CatalogError::RedirectTooDeep)
    }

    /// Read only the addressed frame, rehash it, then assert its decoded identity
    /// and revision match the catalog. A corrupt/stale catalog fails closed.
    pub fn read_record(
        &self,
        root: &Path,
        key: &ArticleKey,
    ) -> Result<Option<PageRecord>, CatalogError> {
        let Some(entry) = self.lookup_entry(key)? else {
            return Ok(None);
        };
        let (record, _) = read_verified_frame(
            root,
            &entry.shard_name,
            entry.offset,
            entry.frame_bytes,
            Some(&entry.frame_sha256),
        )?;
        let (actual_key, title, revision, wikidata, _) = identity(&record);
        if actual_key != entry.key
            || title != entry.title
            || revision != entry.revision_id
            || wikidata != entry.wikidata_id
        {
            return Err(CatalogError::IdentityMismatch);
        }
        Ok(Some(record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_codec::append_record_frame;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiki_model::{Article, Redirect, Revision, ARTICLE_SCHEMA_VERSION};

    fn temp_root() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("wiki-catalog-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn article(id: u64, title: &str) -> PageRecord {
        PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: id,
            },
            revision: Revision {
                revision_id: 1000 + id,
                timestamp: "2026-10-10T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: title.into(),
            display_title: title.into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec!["The blue planet".into()],
            wikidata_id: Some("Q2".into()),
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Earth</article>".into(),
            is_disambiguation: false,
        }))
    }

    #[test]
    fn activated_catalog_opens_read_only_without_creating_or_rewriting_snapshot() {
        let root = temp_root();
        let database = root.join("catalog.sqlite");
        let mut frame = Vec::new();
        append_record_frame(&mut frame, &article(42, "Earth")).unwrap();
        fs::write(root.join("record.shard"), &frame).unwrap();
        let mut writer = SnapshotCatalog::open(&database).unwrap();
        let key = writer
            .insert_verified(&root, "record.shard", 0, frame.len() as u64)
            .unwrap();
        drop(writer);

        let reader = SnapshotCatalog::open_read_only(&database).unwrap();
        assert_eq!(
            reader.lookup_key("enwiki", "earth").unwrap(),
            Some(key.clone())
        );
        assert!(reader.read_record(&root, &key).unwrap().is_some());
        assert!(reader.conn.execute("DELETE FROM records", []).is_err());
        assert!(reader.lookup_entry(&key).unwrap().is_some());
        assert!(SnapshotCatalog::open_read_only(&root.join("missing.sqlite")).is_err());

        let unrelated = root.join("not-a-catalog.sqlite");
        drop(Connection::open(&unrelated).unwrap());
        assert!(matches!(
            SnapshotCatalog::open_read_only(&unrelated),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sqlite_catalog_open_rejects_linked_paths() {
        use std::os::unix::fs::symlink;
        let root = temp_root();
        let database = root.join("catalog.sqlite");
        drop(SnapshotCatalog::open(&database).unwrap());
        let alias = root.join("alias.sqlite");
        symlink(&database, &alias).unwrap();
        assert!(matches!(
            SnapshotCatalog::open_read_only(&alias),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        assert!(matches!(
            SnapshotCatalog::open(&alias),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        fs::remove_file(&alias).unwrap();
        fs::hard_link(&database, &alias).unwrap();
        assert!(matches!(
            SnapshotCatalog::open_read_only(&database),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        fs::remove_file(&alias).unwrap();
        let linked_parent = root.join("linked-parent");
        symlink(&root, &linked_parent).unwrap();
        assert!(matches!(
            SnapshotCatalog::open_read_only(&linked_parent.join("catalog.sqlite")),
            Err(CatalogError::InvalidFrame)
        ));
        assert!(SnapshotCatalog::open_read_only(&database).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shard_names_are_portable_and_reject_windows_device_aliases() {
        for name in [
            "",
            ".",
            "..",
            ".hidden",
            "article.",
            "CON",
            "con.shard",
            "PRN.txt",
            "aux",
            "NUL.bin",
            "COM1",
            "com9.shard",
            "LPT1.bin",
            "lpt9",
            "folder/shard",
            "shard\\name",
            "a:b",
            "a name",
            "bad\u{0000}name",
        ] {
            assert!(
                !valid_shard_name(name),
                "unsafe shard name accepted: {name:?}"
            );
        }
        for name in ["articles-001.shard", "COM0.bin", "LPT0", "CONSOLE.bin"] {
            assert!(valid_shard_name(name), "safe shard name rejected: {name:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_catalog_rejects_symlinked_root_and_hardlinked_frames() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let snapshot = root.join("snapshot");
        fs::create_dir(&snapshot).unwrap();
        let mut frame = Vec::new();
        append_record_frame(&mut frame, &article(42, "Earth")).unwrap();
        fs::write(snapshot.join("articles-001.shard"), &frame).unwrap();

        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        let key = catalog
            .insert_verified(&snapshot, "articles-001.shard", 0, frame.len() as u64)
            .unwrap();
        assert!(catalog.read_record(&snapshot, &key).unwrap().is_some());

        let alias = root.join("symlinked-snapshot");
        symlink(&snapshot, &alias).unwrap();
        assert!(matches!(
            catalog.read_record(&alias, &key),
            Err(CatalogError::InvalidFrame)
        ));
        assert!(matches!(
            catalog.insert_verified(&alias, "articles-001.shard", 0, frame.len() as u64),
            Err(CatalogError::InvalidFrame)
        ));

        let hardlink = root.join("shared-shard");
        fs::hard_link(snapshot.join("articles-001.shard"), &hardlink).unwrap();
        assert!(matches!(
            catalog.read_record(&snapshot, &key),
            Err(CatalogError::InvalidFrame)
        ));
        fs::remove_file(hardlink).unwrap();
        assert!(catalog.read_record(&snapshot, &key).unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_catalog_rejects_symlinked_parent_even_when_root_is_directory() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let actual_parent = root.join("actual-parent");
        let actual_snapshot = actual_parent.join("snapshot");
        fs::create_dir(&actual_parent).unwrap();
        fs::create_dir(&actual_snapshot).unwrap();
        let mut frame = Vec::new();
        append_record_frame(&mut frame, &article(42, "Earth")).unwrap();
        fs::write(actual_snapshot.join("one.shard"), &frame).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        let key = catalog
            .insert_verified(&actual_snapshot, "one.shard", 0, frame.len() as u64)
            .unwrap();

        let alias_parent = root.join("alias-parent");
        symlink(&actual_parent, &alias_parent).unwrap();
        let aliased_snapshot = alias_parent.join("snapshot");
        assert!(fs::symlink_metadata(&aliased_snapshot).unwrap().is_dir());
        assert!(matches!(
            catalog.read_record(&aliased_snapshot, &key),
            Err(CatalogError::InvalidFrame)
        ));
        assert!(matches!(
            catalog.insert_verified(&aliased_snapshot, "one.shard", 0, frame.len() as u64),
            Err(CatalogError::InvalidFrame)
        ));
        assert!(catalog
            .read_record(&actual_snapshot, &key)
            .unwrap()
            .is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn indexed_lookup_reads_exact_one_frame_and_verifies_bytes() {
        let root = temp_root();
        let path = root.join("articles-001.shard");
        let mut shard = Vec::new();
        let first = article(42, "Earth");
        append_record_frame(&mut shard, &first).unwrap();
        let second_offset = shard.len() as u64;
        let redirect = PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 43,
            },
            title: "Planet Earth".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
        });
        append_record_frame(&mut shard, &redirect).unwrap();
        fs::write(&path, &shard).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        let first_key = catalog
            .insert_verified(&root, "articles-001.shard", 0, second_offset)
            .unwrap();
        let second_key = catalog
            .insert_verified(
                &root,
                "articles-001.shard",
                second_offset,
                shard.len() as u64 - second_offset,
            )
            .unwrap();
        assert_eq!(
            catalog.lookup_key("enwiki", "the_blue_planet").unwrap(),
            Some(first_key.clone())
        );
        assert_eq!(
            catalog.lookup_key("enwiki", "planet Earth").unwrap(),
            Some(second_key.clone())
        );
        assert_eq!(catalog.lookup_key("enwiki", "PLANET EARTH").unwrap(), None);
        assert_eq!(
            catalog.lookup_wikidata("enwiki", "Q2").unwrap(),
            vec![first_key.clone()]
        );
        assert_eq!(
            catalog.read_record(&root, &first_key).unwrap(),
            Some(first.clone())
        );
        assert_eq!(
            catalog.read_record(&root, &second_key).unwrap(),
            Some(redirect)
        );
        assert_eq!(
            catalog
                .resolve_title(&root, "enwiki", "Planet Earth")
                .unwrap(),
            Some((first_key.clone(), first.clone()))
        );
        assert!(catalog
            .read_record(
                &root,
                &ArticleKey {
                    project: "enwiki".into(),
                    page_id: 999
                }
            )
            .unwrap()
            .is_none());

        let mut tampered = shard;
        tampered[second_offset as usize + 12] ^= 1;
        fs::write(&path, &tampered).unwrap();
        assert!(matches!(
            catalog.read_record(&root, &second_key),
            Err(CatalogError::DigestMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prefix_title_lookup_verifies_frames_and_redirect_targets() {
        let root = temp_root();
        let mut source = article(42, "Earth");
        if let PageRecord::Article(ref mut article) = source {
            article.aliases.push("Earth science".to_owned());
        }
        let redirect = PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 43,
            },
            title: "Planet Earth".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
        });
        let mut bytes = Vec::new();
        append_record_frame(&mut bytes, &source).unwrap();
        let second_start = bytes.len() as u64;
        append_record_frame(&mut bytes, &redirect).unwrap();
        fs::write(root.join("titles.shard"), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "titles.shard", 0, second_start)
            .unwrap();
        catalog
            .insert_verified(
                &root,
                "titles.shard",
                second_start,
                bytes.len() as u64 - second_start,
            )
            .unwrap();

        for query in ["earth", "Earth_", "Planet_", "The_blue_"] {
            let matches = catalog
                .search_title_prefix(&root, "enwiki", query, 10)
                .unwrap();
            assert_eq!(matches.len(), 1, "{query}");
            assert_eq!(matches[0].key.page_id, 42);
            assert_eq!(matches[0].revision.revision_id, 1042);
        }
        assert!(catalog
            .search_title_prefix(&root, "enwiki", "Missing", 10)
            .unwrap()
            .is_empty());
        assert!(catalog
            .search_title_prefix(&root, "frwiki", "Earth", 5)
            .unwrap()
            .is_empty());
        for bad in ["", " E", "E\u{202e}arth", "E\narth"] {
            assert!(matches!(
                catalog.search_title_prefix(&root, "enwiki", bad, 5),
                Err(CatalogError::InvalidCatalogEntry)
            ));
        }
        assert!(matches!(
            catalog.search_title_prefix(&root, "enwiki", "Earth", 0),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        assert!(matches!(
            catalog.search_title_prefix(&root, "enwiki", "Earth", 101),
            Err(CatalogError::InvalidCatalogEntry)
        ));

        // The immutable redirect and article frames remain valid, but a
        // corrupted derived index must not substitute a different identity.
        catalog
            .conn
            .execute(
                "UPDATE title_index SET page_id=43 WHERE project='enwiki' AND normalized_title='Earth'",
                [],
            )
            .unwrap();
        assert!(matches!(
            catalog.resolve_title(&root, "enwiki", "Earth"),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        assert!(matches!(
            catalog.search_title_prefix(&root, "enwiki", "Earth", 10),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        catalog
            .conn
            .execute(
                "UPDATE title_index SET page_id=42 WHERE project='enwiki' AND normalized_title='Earth'",
                [],
            )
            .unwrap();
        let resolved = catalog
            .resolve_title(&root, "enwiki", "Earth")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.0.page_id, 42);

        bytes[12] ^= 1;
        fs::write(root.join("titles.shard"), &bytes).unwrap();
        assert!(matches!(
            catalog.search_title_prefix(&root, "enwiki", "Earth", 10),
            Err(CatalogError::DigestMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn title_keys_only_fold_initial_character() {
        assert_eq!(title_key("gravity_waves").unwrap(), "Gravity waves");
        assert_eq!(title_key("énergie").unwrap(), "Énergie");
        assert_eq!(title_key("gRAVITY").unwrap(), "GRAVITY");
        assert_ne!(title_key("Gravity").unwrap(), title_key("GRAVITY").unwrap());
        assert!(matches!(
            title_key("  "),
            Err(CatalogError::InvalidCatalogEntry)
        ));
    }

    #[test]
    fn insertion_is_atomic_on_colliding_titles_and_rejects_unsafe_paths() {
        let root = temp_root();
        let mut shard = Vec::new();
        let n = append_record_frame(&mut shard, &article(10, "Earth")).unwrap();
        assert_eq!(n, 0);
        let next = shard.len() as u64;
        append_record_frame(&mut shard, &article(11, "earth")).unwrap();
        fs::write(root.join("shard-1.bin"), &shard).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "shard-1.bin", 0, next)
            .unwrap();
        assert!(matches!(
            catalog.insert_verified(&root, "shard-1.bin", next, shard.len() as u64 - next),
            Err(CatalogError::Sql(_))
        ));
        assert!(catalog
            .lookup_entry(&ArticleKey {
                project: "enwiki".into(),
                page_id: 11
            })
            .unwrap()
            .is_none());
        assert!(matches!(
            catalog.insert_verified(&root, "../shard-1.bin", 0, next),
            Err(CatalogError::UnsafeShardName)
        ));
        assert!(matches!(
            catalog.insert_verified(&root, "shard-1.bin", 0, u64::MAX),
            Err(CatalogError::InvalidFrame)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn redirect_cycles_and_dangling_targets_fail_closed() {
        let root = temp_root();
        let mut shard = Vec::new();
        let first = PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 10,
            },
            title: "Cycle A".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 11,
            },
        });
        let second = PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 11,
            },
            title: "Cycle B".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 10,
            },
        });
        append_record_frame(&mut shard, &first).unwrap();
        let next = shard.len() as u64;
        append_record_frame(&mut shard, &second).unwrap();
        fs::write(root.join("redirects.shard"), &shard).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "redirects.shard", 0, next)
            .unwrap();
        assert!(matches!(
            catalog.resolve_title(&root, "enwiki", "Cycle A"),
            Err(CatalogError::DanglingRedirect)
        ));
        catalog
            .insert_verified(&root, "redirects.shard", next, shard.len() as u64 - next)
            .unwrap();
        assert!(matches!(
            catalog.resolve_title(&root, "enwiki", "Cycle A"),
            Err(CatalogError::RedirectCycle)
        ));
        assert!(catalog
            .resolve_title(&root, "enwiki", "Unknown title")
            .unwrap()
            .is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_shard_coverage_rejects_trailing_bytes_and_missing_frames() {
        let root = temp_root();
        let shard_name = "coverage.shard";
        let first = article(42, "Earth");
        let mut bytes = Vec::new();
        append_record_frame(&mut bytes, &first).unwrap();
        let first_len = bytes.len() as u64;
        let mut second = article(43, "Mars");
        if let PageRecord::Article(ref mut page) = second {
            page.aliases.clear();
            page.wikidata_id = Some("Q111".into());
        }
        append_record_frame(&mut bytes, &second).unwrap();
        fs::write(root.join(shard_name), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, shard_name, 0, first_len)
            .unwrap();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::InvalidFrame)
        ));
        catalog
            .insert_verified(&root, shard_name, first_len, bytes.len() as u64 - first_len)
            .unwrap();
        assert_eq!(catalog.verify_shard_coverage(&root).unwrap(), 2);

        // Missing aliases, forged aliases and cross-record title mappings
        // must all fail activation even when the compressed frames are sound.
        catalog
            .conn
            .execute(
                "DELETE FROM title_index WHERE project='enwiki' AND normalized_title='The blue planet'",
                [],
            )
            .unwrap();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        catalog
            .conn
            .execute(
                "INSERT INTO title_index(project, normalized_title, page_id) VALUES ('enwiki', 'The blue planet', 42)",
                [],
            )
            .unwrap();
        catalog
            .conn
            .execute(
                "INSERT INTO title_index(project, normalized_title, page_id) VALUES ('enwiki', 'Forged alias', 42)",
                [],
            )
            .unwrap();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        catalog
            .conn
            .execute(
                "DELETE FROM title_index WHERE project='enwiki' AND normalized_title='Forged alias'",
                [],
            )
            .unwrap();
        catalog
            .conn
            .execute(
                "UPDATE title_index SET page_id=43 WHERE project='enwiki' AND normalized_title='Earth'",
                [],
            )
            .unwrap();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        catalog
            .conn
            .execute(
                "UPDATE title_index SET page_id=42 WHERE project='enwiki' AND normalized_title='Earth'",
                [],
            )
            .unwrap();
        assert_eq!(catalog.verify_shard_coverage(&root).unwrap(), 2);

        fs::write(root.join(shard_name), [&bytes[..], b"trailing"].concat()).unwrap();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::InvalidFrame)
        ));
        fs::remove_file(root.join(shard_name)).unwrap();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::Io(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expected_shard_set_must_match_catalog_inventory() {
        let root = temp_root();
        let mut bytes = Vec::new();
        append_record_frame(&mut bytes, &article(42, "Earth")).unwrap();
        fs::write(root.join("one.shard"), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "one.shard", 0, bytes.len() as u64)
            .unwrap();
        let expected = BTreeSet::from(["one.shard".to_owned()]);
        assert_eq!(catalog.verify_expected_shards(&root, &expected).unwrap(), 1);
        let incomplete = BTreeSet::new();
        assert!(matches!(
            catalog.verify_expected_shards(&root, &incomplete),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        let extra = BTreeSet::from(["one.shard".to_owned(), "two.shard".to_owned()]);
        assert!(matches!(
            catalog.verify_expected_shards(&root, &extra),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_shard_hashes_require_exact_bytes() {
        let root = temp_root();
        let mut bytes = Vec::new();
        append_record_frame(&mut bytes, &article(42, "Earth")).unwrap();
        fs::write(root.join("manifest.shard"), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "manifest.shard", 0, bytes.len() as u64)
            .unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let mut manifest = std::collections::BTreeMap::from([(
            "manifest.shard".to_owned(),
            (bytes.len() as u64, digest.clone()),
        )]);
        assert_eq!(
            catalog
                .verify_manifest_shard_hashes(&root, &manifest)
                .unwrap(),
            1
        );
        manifest.get_mut("manifest.shard").unwrap().1 = "0".repeat(64);
        assert!(matches!(
            catalog.verify_manifest_shard_hashes(&root, &manifest),
            Err(CatalogError::DigestMismatch)
        ));
        manifest.get_mut("manifest.shard").unwrap().1 = digest;
        manifest.get_mut("manifest.shard").unwrap().0 += 1;
        assert!(matches!(
            catalog.verify_manifest_shard_hashes(&root, &manifest),
            Err(CatalogError::InvalidFrame)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn full_shard_coverage_rejects_empty_catalog() {
        let catalog = SnapshotCatalog::in_memory().unwrap();
        let root = temp_root();
        assert!(matches!(
            catalog.verify_shard_coverage(&root),
            Err(CatalogError::InvalidCatalogEntry)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
