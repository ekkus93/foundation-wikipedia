//! SQLite index for one immutable snapshot's independently compressed records.
//!
//! A catalog is not a source-authenticity proof. Activation must independently
//! verify the full upstream manifest and every required object before use.
//! Each lookup rechecks the exact frame digest and record identity.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use wiki_model::{ArticleKey, ModelError, PageRecord};

use crate::record_codec::{
    decode_record_at, RecordCodecError, MAX_COMPRESSED_RECORD_BYTES,
};

const MAX_FRAME_BYTES: u64 = MAX_COMPRESSED_RECORD_BYTES + 40;

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
}

impl From<rusqlite::Error> for CatalogError {
    fn from(value: rusqlite::Error) -> Self { Self::Sql(value) }
}
impl From<std::io::Error> for CatalogError {
    fn from(value: std::io::Error) -> Self { Self::Io(value) }
}
impl From<RecordCodecError> for CatalogError {
    fn from(value: RecordCodecError) -> Self { Self::Codec(value) }
}
impl From<ModelError> for CatalogError {
    fn from(value: ModelError) -> Self { Self::Model(value) }
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

pub struct SnapshotCatalog {
    conn: Connection,
}

fn valid_shard_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn title_key(title: &str) -> Result<String, CatalogError> {
    let normalized = title.trim().replace('_', " ").to_lowercase();
    if normalized.is_empty() { return Err(CatalogError::InvalidCatalogEntry); }
    Ok(normalized)
}

fn as_i64(value: u64) -> Result<i64, CatalogError> {
    i64::try_from(value).map_err(|_| CatalogError::InvalidCatalogEntry)
}
fn as_u64(value: i64) -> Result<u64, CatalogError> {
    u64::try_from(value).map_err(|_| CatalogError::InvalidCatalogEntry)
}

fn read_verified_frame(
    root: &Path,
    shard_name: &str,
    offset: u64,
    frame_bytes: u64,
    expected_digest: Option<&str>,
) -> Result<(PageRecord, String), CatalogError> {
    if !valid_shard_name(shard_name) { return Err(CatalogError::UnsafeShardName); }
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
        if prior.ino() != opened.ino() || prior.dev() != opened.dev() {
            return Err(CatalogError::InvalidFrame);
        }
    }
    if offset.checked_add(frame_bytes).is_none_or(|end| end > opened.len()) {
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

impl SnapshotCatalog {
    pub fn open(path: &Path) -> Result<Self, CatalogError> {
        Self::initialize(Connection::open(path)?)
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
               ON records(project, wikidata_id);"
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

    pub fn lookup_key(&self, project: &str, title: &str) -> Result<Option<ArticleKey>, CatalogError> {
        let title = title_key(title)?;
        let found: Option<i64> = self.conn.query_row(
            "SELECT page_id FROM title_index WHERE project=?1 AND normalized_title=?2",
            params![project, title],
            |row| row.get(0),
        ).optional()?;
        found.map(|page_id| Ok(ArticleKey { project: project.to_owned(), page_id: as_u64(page_id)? })).transpose()
    }

    pub fn lookup_wikidata(&self, project: &str, item: &str) -> Result<Vec<ArticleKey>, CatalogError> {
        let mut stmt = self.conn.prepare(
            "SELECT page_id FROM records WHERE project=?1 AND wikidata_id=?2 ORDER BY page_id"
        )?;
        let mut matches = Vec::new();
        for row in stmt.query_map(params![project, item], |row| row.get::<_, i64>(0))? {
            matches.push(ArticleKey { project: project.to_owned(), page_id: as_u64(row?)? });
        }
        Ok(matches)
    }

    pub fn lookup_entry(&self, key: &ArticleKey) -> Result<Option<CatalogEntry>, CatalogError> {
        key.validate()?;
        let row: Option<(String, Option<i64>, Option<String>, String, i64, i64, String)> =
            self.conn.query_row(
                "SELECT title,revision_id,wikidata_id,shard_name,frame_offset,frame_bytes,frame_sha256
                   FROM records WHERE project=?1 AND page_id=?2",
                params![key.project, as_i64(key.page_id)?],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))
            ).optional()?;
        row.map(|(title, revision_id, wikidata_id, shard_name, offset, frame_bytes, frame_sha256)| {
            if !valid_shard_name(&shard_name) || frame_sha256.len() != 64
                || !frame_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(CatalogError::InvalidCatalogEntry);
            }
            Ok(CatalogEntry {
                key: key.clone(), title,
                revision_id: revision_id.map(as_u64).transpose()?,
                wikidata_id, shard_name, offset: as_u64(offset)?,
                frame_bytes: as_u64(frame_bytes)?, frame_sha256,
            })
        }).transpose()
    }

    /// Read only the addressed frame, rehash it, then assert its decoded identity
    /// and revision match the catalog. A corrupt/stale catalog fails closed.
    pub fn read_record(&self, root: &Path, key: &ArticleKey) -> Result<Option<PageRecord>, CatalogError> {
        let Some(entry) = self.lookup_entry(key)? else { return Ok(None); };
        let (record, _) = read_verified_frame(
            root, &entry.shard_name, entry.offset, entry.frame_bytes, Some(&entry.frame_sha256),
        )?;
        let (actual_key, title, revision, wikidata, _) = identity(&record);
        if actual_key != entry.key || title != entry.title
            || revision != entry.revision_id || wikidata != entry.wikidata_id
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
    use wiki_model::{Article, Redirect, Revision, ARTICLE_SCHEMA_VERSION};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> std::path::PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("wiki-catalog-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn article(id: u64, title: &str) -> PageRecord {
        PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey { project: "enwiki".into(), page_id: id },
            revision: Revision {
                revision_id: 1000 + id, timestamp: "2026-10-10T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: title.into(), display_title: title.into(), language: "en".into(),
            namespace: 0, aliases: vec!["The blue planet".into()],
            wikidata_id: Some("Q2".into()), lead: vec![], sections: vec![],
            references: vec![], links: vec![], media: vec![],
            rendered_html: "<article>Earth</article>".into(), is_disambiguation: false,
        }))
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
            from: ArticleKey { project: "enwiki".into(), page_id: 43 },
            title: "Planet Earth".into(),
            to: ArticleKey { project: "enwiki".into(), page_id: 42 },
        });
        append_record_frame(&mut shard, &redirect).unwrap();
        fs::write(&path, &shard).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        let first_key = catalog.insert_verified(&root, "articles-001.shard", 0, second_offset).unwrap();
        let second_key = catalog.insert_verified(
            &root, "articles-001.shard", second_offset, shard.len() as u64 - second_offset,
        ).unwrap();
        assert_eq!(catalog.lookup_key("enwiki", "the_blue_planet").unwrap(), Some(first_key.clone()));
        assert_eq!(catalog.lookup_key("enwiki", "PLANET EARTH").unwrap(), Some(second_key.clone()));
        assert_eq!(catalog.lookup_wikidata("enwiki", "Q2").unwrap(), vec![first_key.clone()]);
        assert_eq!(catalog.read_record(&root, &first_key).unwrap(), Some(first));
        assert_eq!(catalog.read_record(&root, &second_key).unwrap(), Some(redirect));
        assert!(catalog.read_record(&root, &ArticleKey { project: "enwiki".into(), page_id: 999 }).unwrap().is_none());

        let mut tampered = shard;
        tampered[second_offset as usize + 12] ^= 1;
        fs::write(&path, &tampered).unwrap();
        assert!(matches!(catalog.read_record(&root, &second_key), Err(CatalogError::DigestMismatch)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn insertion_is_atomic_on_colliding_titles_and_rejects_unsafe_paths() {
        let root = temp_root();
        let mut shard = Vec::new();
        let n = append_record_frame(&mut shard, &article(10, "Earth")).unwrap();
        assert_eq!(n, 0);
        let next = shard.len() as u64;
        append_record_frame(&mut shard, &article(11, "Earth")).unwrap();
        fs::write(root.join("shard-1.bin"), &shard).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog.insert_verified(&root, "shard-1.bin", 0, next).unwrap();
        assert!(matches!(
            catalog.insert_verified(&root, "shard-1.bin", next, shard.len() as u64 - next),
            Err(CatalogError::Sql(_))
        ));
        assert!(catalog.lookup_entry(&ArticleKey { project: "enwiki".into(), page_id: 11 }).unwrap().is_none());
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
}
