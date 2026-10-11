//! Derived, rebuildable offline full-text search over canonical articles.
//! Source authority and complete pack membership are verified elsewhere.

use crate::catalog::{CatalogError, SnapshotCatalog};
use rusqlite::{params, Connection, Transaction};
use std::collections::BTreeSet;
use std::path::Path;
use wiki_model::{Article, ArticleKey, Block, BlockContent, ModelError, PageRecord, Section};

const MAX_ARTICLES: usize = 100_000;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_QUERY_BYTES: usize = 8192;

#[derive(Debug)]
pub enum SearchError {
    Sql(rusqlite::Error),
    Model(ModelError),
    Catalog(CatalogError),
    Duplicate(ArticleKey),
    TooManyArticles,
    TooMuchText(ArticleKey),
    InvalidIdentity,
    StaleIndex,
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

impl OfflineSearchIndex {
    pub fn open(path: &std::path::Path) -> Result<Self, SearchError> {
        Self::initialize(Connection::open(path)?)
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

    /// Rebuild directly from a verified immutable catalog's individual frames.
    /// Redirect records are skipped, and a corrupt late frame rolls back the
    /// derived index rather than publishing an incomplete search corpus.
    /// Source authenticity and completeness must be checked by the caller.
    pub fn rebuild_from_catalog(
        &mut self,
        catalog: &SnapshotCatalog,
        root: &Path,
    ) -> Result<(), SearchError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM offline_article_fts", [])?;
        let mut seen = BTreeSet::new();
        let mut count = 0;
        catalog.visit_records(root, |record| {
            if let PageRecord::Article(article) = record {
                index_article(&tx, &mut seen, count, *article)?;
                count += 1;
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
