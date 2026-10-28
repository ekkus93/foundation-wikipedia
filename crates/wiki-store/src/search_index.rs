//! Derived, rebuildable offline full-text search over canonical articles.
//! Source authority and complete pack membership are verified elsewhere.

use crate::catalog::{CatalogError, SnapshotCatalog};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use wiki_model::{Article, ArticleKey, Block, BlockContent, ModelError, PageRecord, Section};

static NEXT_REBUILD_TEMP: AtomicU64 = AtomicU64::new(0);

const MAX_ARTICLES: usize = 100_000;
const MAX_REDIRECTS: usize = 500_000;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_QUERY_BYTES: usize = 8192;

#[derive(Debug)]
pub enum SearchError {
    Sql(rusqlite::Error),
    Io(io::Error),
    UnsafePath,
    Model(ModelError),
    Catalog(CatalogError),
    Duplicate(ArticleKey),
    TooManyArticles,
    TooManyRedirects,
    TooMuchText(ArticleKey),
    InvalidIdentity,
    StaleIndex,
}
impl From<io::Error> for SearchError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<CatalogError> for SearchError {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}
impl From<rusqlite::Error> for SearchError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub key: ArticleKey,
    pub revision_id: u64,
    pub title: String,
}

pub struct OfflineSearchIndex {
    conn: Connection,
}

fn append_blocks(blocks: &[Block], text: &mut String) {
    for block in blocks {
        match &block.content {
            BlockContent::Paragraph(s) | BlockContent::Quote(s) => {
                text.push_str(s);
                text.push(' ');
            }
            BlockContent::List(items) => {
                for s in items {
                    text.push_str(s);
                    text.push(' ');
                }
            }
            BlockContent::Table(rows) => {
                for s in rows.iter().flatten() {
                    text.push_str(s);
                    text.push(' ');
                }
            }
            BlockContent::Math { source, .. } => {
                text.push_str(source);
                text.push(' ');
            }
            BlockContent::Infobox(fields) => {
                for (key, value) in fields {
                    text.push_str(key);
                    text.push(' ');
                    text.push_str(value);
                    text.push(' ');
                }
            }
            BlockContent::Footnote(note) => {
                text.push_str(&note.text);
                text.push(' ');
            }
            BlockContent::HtmlFallback(_) | BlockContent::Media { .. } => {}
        }
    }
}

fn append_sections(sections: &[Section], text: &mut String) {
    for section in sections {
        text.push_str(&section.heading);
        text.push(' ');
        append_blocks(&section.blocks, text);
        append_sections(&section.subsections, text);
    }
}

fn query_expression(query: &str) -> Option<String> {
    if query.len() > MAX_QUERY_BYTES {
        return None;
    }
    let tokens: BTreeSet<_> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(32)
        .map(str::to_lowercase)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|term| format!("\"{term}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

fn index_article(
    tx: &Transaction<'_>,
    seen: &mut BTreeSet<(String, u64)>,
    position: usize,
    article: Article,
) -> Result<(), SearchError> {
    if position >= MAX_ARTICLES {
        return Err(SearchError::TooManyArticles);
    }
    article.validate().map_err(SearchError::Model)?;
    if !seen.insert((article.key.project.clone(), article.key.page_id)) {
        return Err(SearchError::Duplicate(article.key.clone()));
    }
    let mut text = String::new();
    append_blocks(&article.lead, &mut text);
    append_sections(&article.sections, &mut text);
    // Searchable title variants are canonical article metadata, not separate
    // redirect records. Preserve one exact revision identity per result.
    text.push_str(&article.display_title);
    text.push(' ');
    for alias in &article.aliases {
        text.push_str(alias);
        text.push(' ');
    }
    if text.len().saturating_add(article.title.len()) > MAX_TEXT_BYTES {
        return Err(SearchError::TooMuchText(article.key.clone()));
    }
    let page = i64::try_from(article.key.page_id).map_err(|_| SearchError::InvalidIdentity)?;
    let revision =
        i64::try_from(article.revision.revision_id).map_err(|_| SearchError::InvalidIdentity)?;
    tx.execute(
        "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
         VALUES (?1,?2,?3,?4,?5)",
        params![article.key.project, page, revision, article.title, text],
    )?;
    Ok(())
}

fn validate_index_path(path: &Path) -> Result<(), SearchError> {
    // This is a derived private database, never an arbitrary caller-supplied
    // SQLite pathname. Validate every ancestor, not only the direct parent.
    // The directory must be exclusively controlled by the caller throughout
    // opening/replacement; this cannot protect against concurrent renames.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    for ancestor in parent.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(ancestor)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(SearchError::UnsafePath);
        }
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            Err(SearchError::UnsafePath)
        }
        Ok(meta) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                // Never read/write a multiply-linked database or accept an
                // existing DB accessible to other local users.
                if meta.nlink() != 1 || meta.mode() & 0o077 != 0 {
                    return Err(SearchError::UnsafePath);
                }
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SearchError::Io(error)),
    }
}

impl OfflineSearchIndex {
    /// Read a published FTS database without creating or migrating the index.
    pub fn open_read_only(path: &Path) -> Result<Self, SearchError> {
        validate_index_path(path)?;
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.execute_batch("PRAGMA query_only=ON;")?;
        let schema: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='offline_article_fts'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if !schema.is_some_and(|ddl| {
            ddl.starts_with("CREATE VIRTUAL TABLE") && ddl.contains(" USING fts5(")
        }) {
            return Err(SearchError::StaleIndex);
        }
        Ok(Self { conn })
    }

    /// Open an existing derived index only after checking complete article
    /// identity membership against every digest-verified catalog record.
    /// This does not authenticate the source publication, validate required
    /// media, or prove that FTS token postings match canonical body text.
    /// The caller must keep the snapshot immutable for the reader's lifetime.
    pub fn open_verified_read_only(
        path: &Path,
        catalog: &SnapshotCatalog,
        root: &Path,
    ) -> Result<Self, SearchError> {
        let index = Self::open_read_only(path)?;
        index.verify_catalog_membership(catalog, root)?;
        Ok(index)
    }

    pub fn open(path: &std::path::Path) -> Result<Self, SearchError> {
        validate_index_path(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            match fs::symlink_metadata(path) {
                Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
                    return Err(SearchError::UnsafePath);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)?;
                }
                Err(error) => return Err(SearchError::Io(error)),
            }
        }
        Self::initialize(Connection::open(path)?)
    }

    /// Rebuild a derived SQLite FTS5 file from verified catalog frames, even
    /// when the existing index is corrupt or cannot be opened.
    ///
    /// Call only with exclusive access to a private writable directory and no
    /// active SQLite connections to `path`. A fresh staged database is fully
    /// built and fsynced before replacing the existing pathname. Any source,
    /// SQLite, or pre-publication I/O failure leaves the prior file untouched.
    /// The caller must independently authenticate and verify the snapshot.
    pub fn replace_persisted_from_catalog(
        path: &Path,
        catalog: &SnapshotCatalog,
        root: &Path,
    ) -> Result<(), SearchError> {
        validate_index_path(path)?;
        let parent = path.parent().ok_or(SearchError::UnsafePath)?;
        let (staged, file) = (0..32)
            .find_map(|_| {
                let candidate = parent.join(format!(
                    ".offline-fts.{}.{}.sqlite",
                    std::process::id(),
                    NEXT_REBUILD_TEMP.fetch_add(1, Ordering::Relaxed)
                ));
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                // Derived FTS content can include private offline articles.
                // Never expose a staged (or published) index via a permissive umask.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(&candidate) {
                    Ok(file) => Some(Ok((candidate, file))),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                    Err(error) => Some(Err(SearchError::Io(error))),
                }
            })
            .ok_or(SearchError::UnsafePath)??;
        drop(file);
        let result = (|| -> Result<(), SearchError> {
            {
                let mut index = Self::open(&staged)?;
                index.rebuild_from_catalog(catalog, root)?;
                // Audit the staged index before it can replace a previously
                // usable file; publication must not rely on query-hit checks.
                index.verify_catalog_membership(catalog, root)?;
            }
            File::open(&staged)?.sync_all()?;
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            // Do not publish a partially indexed SQLite file. The candidate
            // was closed successfully before this atomic same-directory swap.
            fs::rename(&staged, path)?;
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(staged);
        }
        result
    }
    /// Publish a rebuilt search index only after verifying complete shard
    /// coverage and SHA-256 digests against an independently authenticated
    /// manifest. This does NOT authenticate the supplied manifest itself.
    ///
    /// Keep the source snapshot immutable and hold exclusive ownership of the
    /// destination directory throughout verification and publication.
    /// Catalog frames alone cannot prove that unindexed shard bytes are absent.
    pub fn replace_persisted_from_verified_shards(
        path: &Path,
        catalog: &SnapshotCatalog,
        root: &Path,
        expected_shards: &BTreeMap<String, (u64, String)>,
    ) -> Result<(), SearchError> {
        validate_index_path(path)?;
        catalog.verify_manifest_shard_hashes(root, expected_shards)?;
        Self::replace_persisted_from_catalog(path, catalog, root)
    }

    pub fn in_memory() -> Result<Self, SearchError> {
        Self::initialize(Connection::open_in_memory()?)
    }
    fn initialize(conn: Connection) -> Result<Self, SearchError> {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS offline_article_fts USING fts5(
                project UNINDEXED, page_id UNINDEXED, revision_id UNINDEXED,
                title, body, tokenize='unicode61');",
        )?;
        Ok(Self { conn })
    }

    /// All-or-nothing replacement retains the old index if any input fails.
    pub fn rebuild(&mut self, articles: &[Article]) -> Result<(), SearchError> {
        self.rebuild_streaming(articles.iter().cloned().map(Ok))
    }

    /// Consume one canonical article at a time without retaining the corpus.
    /// A source read error or late invalid article rolls back the entire index.
    pub fn rebuild_streaming<I>(&mut self, articles: I) -> Result<(), SearchError>
    where
        I: IntoIterator<Item = Result<Article, SearchError>>,
    {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM offline_article_fts", [])?;
        let mut seen = BTreeSet::new();
        for (position, article) in articles.into_iter().enumerate() {
            index_article(&tx, &mut seen, position, article?)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rebuild directly from digest/identity-verified catalog frames.
    /// Two streaming passes allow redirect titles to resolve to the canonical
    /// installed article, never to a standalone redirect search result.
    ///
    /// All article indexing, alias enrichment and redirect resolution share
    /// a single transaction. Any late corruption, loop, dangling reference or
    /// resource limit aborts, preserving the previous complete derived index.
    /// Source authenticity and complete snapshot membership are caller duties.
    pub fn rebuild_from_catalog(
        &mut self,
        catalog: &SnapshotCatalog,
        root: &Path,
    ) -> Result<(), SearchError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM offline_article_fts", [])?;
        let mut seen = BTreeSet::new();
        let mut rows = BTreeMap::new();
        let mut article_count = 0;
        // First pass: index verified articles and record their FTS rowids.
        catalog.visit_records(root, |record| {
            if let PageRecord::Article(article) = record {
                let key = article.key.clone();
                let revision = article.revision.revision_id;
                index_article(&tx, &mut seen, article_count, *article)?;
                rows.insert(
                    (key.project, key.page_id),
                    (tx.last_insert_rowid(), revision),
                );
                article_count += 1;
            }
            Ok::<(), SearchError>(())
        })?;
        // Second pass: verified redirect names join only to an indexed article.
        // Never produce a second result for an alias or trust a redirect
        // without reading every link of its exact catalog chain.
        let mut redirect_count = 0;
        catalog.visit_records(root, |record| {
            if let PageRecord::Redirect(redirect) = record {
                if redirect_count >= MAX_REDIRECTS {
                    return Err(SearchError::TooManyRedirects);
                }
                redirect_count += 1;
                if redirect.title.len() > MAX_TEXT_BYTES {
                    return Err(SearchError::TooMuchText(redirect.from));
                }
                let (target, resolved) = catalog
                    .resolve_title(root, &redirect.from.project, &redirect.title)?
                    .ok_or(SearchError::StaleIndex)?;
                let PageRecord::Article(article) = resolved else {
                    return Err(SearchError::StaleIndex);
                };
                let (rowid, indexed_revision) = rows
                    .get(&(target.project.clone(), target.page_id))
                    .ok_or(SearchError::StaleIndex)?;
                if *indexed_revision != article.revision.revision_id {
                    return Err(SearchError::StaleIndex);
                }
                let (body_size, title_size): (i64, i64) = tx.query_row(
                    "SELECT length(CAST(body AS BLOB)), length(CAST(title AS BLOB))
                     FROM offline_article_fts WHERE rowid=?1",
                    params![rowid],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                let bytes = usize::try_from(body_size)
                    .ok()
                    .zip(usize::try_from(title_size).ok())
                    .and_then(|(body, title)| {
                        body.checked_add(title)?
                            .checked_add(1)?
                            .checked_add(redirect.title.len())
                    })
                    .ok_or(SearchError::InvalidIdentity)?;
                if bytes > MAX_TEXT_BYTES {
                    return Err(SearchError::TooMuchText(target));
                }
                tx.execute(
                    "UPDATE offline_article_fts SET body=body || ' ' || ?1 WHERE rowid=?2",
                    params![redirect.title, rowid],
                )?;
            }
            Ok::<(), SearchError>(())
        })?;
        tx.commit()?;
        Ok(())
    }

    /// Audit every derived FTS row against the complete immutable catalog,
    /// including editions not represented by a particular search query.
    ///
    /// A matching search result alone cannot detect missing indexed articles.
    /// Stream both inventories in stable project/page-ID order so an omitted,
    /// duplicated, foreign, or stale row fails closed without materializing
    /// the corpus. Every catalog frame (including redirects) is reverified.
    /// This checks identity coverage, not FTS token postings, source
    /// authenticity, or required offline media completeness.
    pub fn verify_catalog_membership(
        &self,
        catalog: &SnapshotCatalog,
        root: &Path,
    ) -> Result<usize, SearchError> {
        let mut stmt = self.conn.prepare(
            "SELECT project,page_id,revision_id,title FROM offline_article_fts
             ORDER BY project, CAST(page_id AS INTEGER), rowid",
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0usize;
        catalog.visit_records(root, |record| {
            if let PageRecord::Article(article) = record {
                if count >= MAX_ARTICLES {
                    return Err(SearchError::TooManyArticles);
                }
                let row = rows.next()?.ok_or(SearchError::StaleIndex)?;
                let project: String = row.get(0)?;
                let page_id: i64 = row.get(1)?;
                let revision_id: i64 = row.get(2)?;
                let title: String = row.get(3)?;
                if project != article.key.project
                    || i64::try_from(article.key.page_id).ok() != Some(page_id)
                    || i64::try_from(article.revision.revision_id).ok() != Some(revision_id)
                    || title != article.title
                {
                    return Err(SearchError::StaleIndex);
                }
                count += 1;
            }
            Ok::<(), SearchError>(())
        })?;
        if rows.next()?.is_some() {
            return Err(SearchError::StaleIndex);
        }
        Ok(count)
    }

    /// Resolve a search hit against the immutable catalog, refusing to open
    /// a different revision (or a redirect) from the one actually indexed.
    /// A stale hit requires a derived-index rebuild, never a silent fallback.
    pub fn resolve_hit(
        &self,
        catalog: &SnapshotCatalog,
        root: &Path,
        hit: &SearchHit,
    ) -> Result<Article, SearchError> {
        match catalog.read_record(root, &hit.key)? {
            Some(PageRecord::Article(article))
                if article.revision.revision_id == hit.revision_id
                    && article.title == hit.title =>
            {
                Ok(*article)
            }
            _ => Err(SearchError::StaleIndex),
        }
    }

    /// Resolve bounded offline search results to exact indexed article
    /// revisions. Any stale identity or corrupt catalog frame fails the
    /// entire operation, rather than returning a partial result set.
    ///
    /// Publisher trust and snapshot completeness must be verified separately.
    pub fn search_verified(
        &self,
        catalog: &SnapshotCatalog,
        root: &Path,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Article>, SearchError> {
        self.search(query, limit)?
            .iter()
            .map(|hit| self.resolve_hit(catalog, root, hit))
            .collect()
    }

    /// Full-text search confined to one validated Wikipedia edition.
    /// The SQLite WHERE clause applies the project filter before ORDER BY
    /// and LIMIT; filtering global search hits afterward is unsafe.
    pub fn search_project(
        &self,
        project: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, SearchError> {
        ArticleKey {
            project: project.to_owned(),
            page_id: 1,
        }
        .validate()
        .map_err(|_| SearchError::InvalidIdentity)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(expression) = query_expression(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT project,page_id,revision_id,title FROM offline_article_fts
             WHERE offline_article_fts MATCH ?1 AND project=?2
             ORDER BY bm25(offline_article_fts, 0.0, 0.0, 0.0, 8.0, 1.0),
                      CAST(page_id AS INTEGER) LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![expression, project, limit.min(100) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (actual_project, page, revision, title) = row?;
            if actual_project != project {
                return Err(SearchError::InvalidIdentity);
            }
            let key = ArticleKey {
                project: actual_project,
                page_id: u64::try_from(page).map_err(|_| SearchError::InvalidIdentity)?,
            };
            key.validate().map_err(|_| SearchError::InvalidIdentity)?;
            let revision_id = u64::try_from(revision).map_err(|_| SearchError::InvalidIdentity)?;
            if revision_id == 0 {
                return Err(SearchError::InvalidIdentity);
            }
            hits.push(SearchHit {
                key,
                revision_id,
                title,
            });
        }
        Ok(hits)
    }

    /// Resolve project-scoped hits against digest-verified immutable frames.
    pub fn search_verified_project(
        &self,
        catalog: &SnapshotCatalog,
        root: &Path,
        project: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Article>, SearchError> {
        self.search_project(project, query, limit)?
            .iter()
            .map(|hit| self.resolve_hit(catalog, root, hit))
            .collect()
    }

    /// Local-only title-boosted lexical ranking, at most 100 results.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(expression) = query_expression(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT project,page_id,revision_id,title FROM offline_article_fts
             WHERE offline_article_fts MATCH ?1
             ORDER BY bm25(offline_article_fts, 0.0, 0.0, 0.0, 8.0, 1.0),
                      project, CAST(page_id AS INTEGER) LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![expression, limit.min(100) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (project, page, revision, title) = row?;
            let key = ArticleKey {
                project,
                page_id: u64::try_from(page).map_err(|_| SearchError::InvalidIdentity)?,
            };
            key.validate().map_err(|_| SearchError::InvalidIdentity)?;
            let revision_id = u64::try_from(revision).map_err(|_| SearchError::InvalidIdentity)?;
            if revision_id == 0 {
                return Err(SearchError::InvalidIdentity);
            }
            hits.push(SearchHit {
                key,
                revision_id,
                title,
            });
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn complete_catalog_membership_audit_rejects_missing_forged_and_stale_fts_rows() {
        use crate::record_codec::append_record_frame;
        use wiki_model::{Revision, ARTICLE_SCHEMA_VERSION};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-fts-membership-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let mut bytes = Vec::new();
        let mut frames = Vec::new();
        // Shard insertion order differs from the SQLite catalog key order.
        // Numeric page-ID ordering must not be confused with FTS text order.
        for (project, page_id, title) in [
            ("enwiki", 10, "Mars"),
            ("frwiki", 1, "Terre"),
            ("enwiki", 2, "Earth"),
        ] {
            let record = PageRecord::Article(Box::new(Article {
                schema_version: ARTICLE_SCHEMA_VERSION,
                key: ArticleKey {
                    project: project.into(),
                    page_id,
                },
                revision: Revision {
                    revision_id: 1000 + page_id,
                    timestamp: "2026-10-28T00:00:00Z".into(),
                    content_sha256: "a".repeat(64),
                },
                title: title.into(),
                display_title: title.into(),
                language: "en".into(),
                namespace: 0,
                aliases: vec![],
                wikidata_id: None,
                lead: vec![],
                sections: vec![],
                references: vec![],
                links: vec![],
                media: vec![],
                rendered_html: format!("<article>{title}</article>"),
                is_disambiguation: false,
            }));
            let start = bytes.len() as u64;
            append_record_frame(&mut bytes, &record).unwrap();
            frames.push((start, bytes.len() as u64 - start));
        }
        fs::write(root.join("audit.shard"), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::open(&root.join("catalog.sqlite")).unwrap();
        for (offset, length) in frames {
            catalog
                .insert_verified(&root, "audit.shard", offset, length)
                .unwrap();
        }
        let mut index = OfflineSearchIndex::in_memory().unwrap();
        index.rebuild_from_catalog(&catalog, &root).unwrap();
        assert_eq!(index.verify_catalog_membership(&catalog, &root).unwrap(), 3);

        index
            .conn
            .execute("DELETE FROM offline_article_fts WHERE title='Mars'", [])
            .unwrap();
        assert!(matches!(
            index.verify_catalog_membership(&catalog, &root),
            Err(SearchError::StaleIndex)
        ));
        index.rebuild_from_catalog(&catalog, &root).unwrap();

        index
            .conn
            .execute(
                "UPDATE offline_article_fts SET revision_id=999 WHERE title='Terre'",
                [],
            )
            .unwrap();
        assert!(matches!(
            index.verify_catalog_membership(&catalog, &root),
            Err(SearchError::StaleIndex)
        ));
        index.rebuild_from_catalog(&catalog, &root).unwrap();

        index
            .conn
            .execute(
                "UPDATE offline_article_fts SET title='Wrong' WHERE title='Earth'",
                [],
            )
            .unwrap();
        assert!(matches!(
            index.verify_catalog_membership(&catalog, &root),
            Err(SearchError::StaleIndex)
        ));
        index.rebuild_from_catalog(&catalog, &root).unwrap();

        index
            .conn
            .execute(
                "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                 VALUES ('enwiki',2,1002,'Earth','earth')",
                [],
            )
            .unwrap();
        assert!(matches!(
            index.verify_catalog_membership(&catalog, &root),
            Err(SearchError::StaleIndex)
        ));
        index.rebuild_from_catalog(&catalog, &root).unwrap();

        bytes[12] ^= 1;
        fs::write(root.join("audit.shard"), &bytes).unwrap();
        assert!(matches!(
            index.verify_catalog_membership(&catalog, &root),
            Err(SearchError::Catalog(CatalogError::DigestMismatch))
        ));
        drop(index);
        drop(catalog);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn published_index_verified_open_rejects_omitted_rows_and_corrupt_frames() {
        use crate::record_codec::append_record_frame;
        use wiki_model::{Revision, ARTICLE_SCHEMA_VERSION};

        let root = std::env::temp_dir().join(format!(
            "wiki-fts-verified-open-{}-{}",
            std::process::id(),
            NEXT_REBUILD_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let record = PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 1042,
                timestamp: "2026-10-28T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Earth".into(),
            display_title: "Earth".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Earth</article>".into(),
            is_disambiguation: false,
        }));
        let mut bytes = Vec::new();
        append_record_frame(&mut bytes, &record).unwrap();
        fs::write(root.join("verified.shard"), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::open(&root.join("catalog.sqlite")).unwrap();
        catalog
            .insert_verified(&root, "verified.shard", 0, bytes.len() as u64)
            .unwrap();
        let path = root.join("derived.sqlite");
        OfflineSearchIndex::replace_persisted_from_catalog(&path, &catalog, &root).unwrap();

        let reader = OfflineSearchIndex::open_verified_read_only(&path, &catalog, &root).unwrap();
        let articles = reader
            .search_verified_project(&catalog, &root, "enwiki", "Earth", 10)
            .unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].key.page_id, 42);
        drop(reader);

        // A plausible, still-queryable FTS schema is not proof that every
        // installed article has a corresponding indexed row.
        {
            let writer = OfflineSearchIndex::open(&path).unwrap();
            writer
                .conn
                .execute("DELETE FROM offline_article_fts", [])
                .unwrap();
        }
        assert!(matches!(
            OfflineSearchIndex::open_verified_read_only(&path, &catalog, &root),
            Err(SearchError::StaleIndex)
        ));

        // Rebuilding the derived index from immutable bytes recovers without
        // re-downloading source articles or their media.
        OfflineSearchIndex::replace_persisted_from_catalog(&path, &catalog, &root).unwrap();
        assert!(OfflineSearchIndex::open_verified_read_only(&path, &catalog, &root).is_ok());

        bytes[12] ^= 1;
        fs::write(root.join("verified.shard"), &bytes).unwrap();
        assert!(matches!(
            OfflineSearchIndex::open_verified_read_only(&path, &catalog, &root),
            Err(SearchError::Catalog(CatalogError::DigestMismatch))
        ));
        drop(catalog);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn published_index_is_read_only_and_missing_schema_fails_closed() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-fts-readonly-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let database = root.join("derived.sqlite");
        {
            let writer = OfflineSearchIndex::open(&database).unwrap();
            writer
                .conn
                .execute(
                    "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                     VALUES ('enwiki',42,1042,'Earth','planet ocean')",
                    [],
                )
                .unwrap();
        }
        let reader = OfflineSearchIndex::open_read_only(&database).unwrap();
        let hits = reader.search_project("enwiki", "ocean", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key.page_id, 42);
        assert!(reader
            .conn
            .execute("DELETE FROM offline_article_fts", [])
            .is_err());
        assert!(OfflineSearchIndex::open_read_only(&root.join("missing.sqlite")).is_err());
        let unrelated = root.join("not-fts.sqlite");
        drop(Connection::open(&unrelated).unwrap());
        // A foreign SQLite file must pass the private-file permission gate
        // before we can assert the separate missing-FTS-schema error.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&unrelated, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(
            OfflineSearchIndex::open_read_only(&unrelated),
            Err(SearchError::StaleIndex)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_filter_precedes_fts_limit_and_ignores_foreign_corruption() {
        let index = OfflineSearchIndex::in_memory().unwrap();
        for page in 1..=110 {
            index
                .conn
                .execute(
                    "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                     VALUES ('arwiki',?1,42,'Needle','needle')",
                    params![page],
                )
                .unwrap();
        }
        index
            .conn
            .execute(
                "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                 VALUES ('enwiki',999,42,'Quiet','needle')",
                [],
            )
            .unwrap();
        let global = index.search("needle", 100).unwrap();
        assert_eq!(global.len(), 100);
        assert!(global.iter().all(|hit| hit.key.project == "arwiki"));
        let en = index.search_project("enwiki", "needle", 100).unwrap();
        assert_eq!(en.len(), 1);
        assert_eq!(en[0].key.page_id, 999);
        assert_eq!(en[0].revision_id, 42);
        assert!(index
            .search_project("enwiki", "needle", 0)
            .unwrap()
            .is_empty());
        assert!(index
            .search_project("missingwiki", "needle", 100)
            .unwrap()
            .is_empty());
        assert!(matches!(
            index.search_project("ENWIKI", "needle", 10),
            Err(SearchError::InvalidIdentity)
        ));
        // Invalid foreign-edition row identities must not poison valid
        // edition-scoped search, but are still rejected globally.
        index
            .conn
            .execute(
                "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                 VALUES ('arwiki',0,42,'Needle','needle')",
                [],
            )
            .unwrap();
        assert_eq!(
            index.search_project("enwiki", "needle", 10).unwrap().len(),
            1
        );
        assert!(matches!(
            index.search("needle", 100),
            Err(SearchError::InvalidIdentity)
        ));
    }

    #[test]
    fn rejects_corrupted_fts_page_revision_and_project_identities() {
        let index = OfflineSearchIndex::in_memory().unwrap();
        for (project, page, revision) in [
            ("enwiki", 0, 123),
            ("enwiki", 42, 0),
            ("ENWIKI", 42, 123),
            ("bad/project", 42, 123),
            ("enwiki", -1, 123),
            ("enwiki", 42, -1),
        ] {
            index
                .conn
                .execute("DELETE FROM offline_article_fts", [])
                .unwrap();
            index
                .conn
                .execute(
                    "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![project, page, revision, "Needle", "needle"],
                )
                .unwrap();
            assert!(matches!(
                index.search("needle", 10),
                Err(SearchError::InvalidIdentity)
            ));
        }
        index
            .conn
            .execute("DELETE FROM offline_article_fts", [])
            .unwrap();
        index
            .conn
            .execute(
                "INSERT INTO offline_article_fts(project,page_id,revision_id,title,body)
                 VALUES (?1,?2,?3,?4,?5)",
                params!["enwiki", 42, 123, "Needle", "needle"],
            )
            .unwrap();
        let results = index.search("needle", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key.page_id, 42);
        assert_eq!(results[0].revision_id, 123);
    }
}
