//! Derived, rebuildable offline full-text search over canonical articles.
//! Source authority and complete pack membership are verified elsewhere.

use crate::catalog::{CatalogError, SnapshotCatalog};
use rusqlite::{params, Connection, Transaction};
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
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
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
             ORDER BY bm25(offline_article_fts, 4.0, 1.0),
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
            hits.push(SearchHit {
                key: ArticleKey {
                    project,
                    page_id: u64::try_from(page).map_err(|_| SearchError::InvalidIdentity)?,
                },
                revision_id: u64::try_from(revision).map_err(|_| SearchError::InvalidIdentity)?,
                title,
            });
        }
        Ok(hits)
    }
}
