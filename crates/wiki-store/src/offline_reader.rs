//! Read-only offline article navigation over one caller-verified snapshot.
//!
//! This service uses no network APIs. A missing page returns `None`, while
//! corrupt frames and dangling redirects return errors, never a stale article.
//! Source-publisher authentication and pack activation are caller obligations.

use std::path::Path;

use wiki_model::{Article, ArticleKey, PageRecord};

use crate::catalog::{CatalogError, SnapshotCatalog};
use crate::search_index::{OfflineSearchIndex, SearchError};

#[derive(Debug)]
pub enum ReaderError {
    Catalog(CatalogError),
    Search(SearchError),
    StaleRevision { expected: u64, actual: u64 },
}

impl From<CatalogError> for ReaderError {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl From<SearchError> for ReaderError {
    fn from(error: SearchError) -> Self {
        Self::Search(error)
    }
}

/// All operations are local and read-only, scoped to a single immutable
/// snapshot and its derived FTS index. Never mix a newer online revision into
/// an installed offline revision through this interface.
pub struct OfflineReader<'a> {
    catalog: &'a SnapshotCatalog,
    index: &'a OfflineSearchIndex,
    root: &'a Path,
}

impl<'a> OfflineReader<'a> {
    pub fn new(
        catalog: &'a SnapshotCatalog,
        index: &'a OfflineSearchIndex,
        root: &'a Path,
    ) -> Self {
        Self {
            catalog,
            index,
            root,
        }
    }

    /// Open an installed page by stable project/page ID, including redirects.
    /// Only a genuinely absent key reports `None`.
    pub fn open_page(&self, key: &ArticleKey) -> Result<Option<Article>, ReaderError> {
        match self.catalog.read_record(self.root, key)? {
            None => Ok(None),
            Some(PageRecord::Article(article)) => Ok(Some(*article)),
            Some(PageRecord::Redirect(redirect)) => {
                // Redirect traversal is bounded and cycle-checked in catalog.
                let resolved = self
                    .catalog
                    .resolve_title(self.root, &key.project, &redirect.title)?
                    .ok_or(CatalogError::DanglingRedirect)?;
                Ok(Some(Self::require_article(resolved.1)?))
            }
        }
    }

    /// Open a page only when its installed canonical revision still matches
    /// the recorded revision. Redirects resolve to their target revision.
    /// A changed revision is an error, never a silent citation relocation.
    pub fn open_exact_revision(
        &self,
        key: &ArticleKey,
        expected: u64,
    ) -> Result<Option<Article>, ReaderError> {
        self.open_page(key)?
            .map(|article| {
                let actual = article.revision.revision_id;
                if actual != expected {
                    Err(ReaderError::StaleRevision { expected, actual })
                } else {
                    Ok(article)
                }
            })
            .transpose()
    }

    /// Open a canonical title or installed redirect alias without requesting
    /// the network, returning `None` for an uninstalled title.
    pub fn open_title(&self, project: &str, title: &str) -> Result<Option<Article>, ReaderError> {
        self.catalog
            .resolve_title(self.root, project, title)?
            .map(|(_, record)| Self::require_article(record))
            .transpose()
    }

    /// Search installed articles, verifying all matched immutable frames
    /// before returning any article body to the caller.
    pub fn search_articles(&self, query: &str, limit: usize) -> Result<Vec<Article>, ReaderError> {
        Ok(self
            .index
            .search_verified(self.catalog, self.root, query, limit)?)
    }

    fn require_article(record: PageRecord) -> Result<Article, ReaderError> {
        match record {
            PageRecord::Article(article) => Ok(*article),
            PageRecord::Redirect(_) => Err(CatalogError::DanglingRedirect.into()),
        }
    }
}
