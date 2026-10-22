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

    /// Prefix-search installed canonical titles, aliases and redirects without
    /// requiring a full-text index rebuild or making any network request.
    /// Every returned article is read from a digest-verified immutable frame.
    /// Callers must separately verify snapshot provenance and mandatory media.
    pub fn search_title_prefix(
        &self,
        project: &str,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<Article>, ReaderError> {
        Ok(self
            .catalog
            .search_title_prefix(self.root, project, prefix, limit)?)
    }

    fn require_article(record: PageRecord) -> Result<Article, ReaderError> {
        match record {
            PageRecord::Article(article) => Ok(*article),
            PageRecord::Redirect(_) => Err(CatalogError::DanglingRedirect.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_codec::append_record_frame;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiki_model::{Redirect, Revision, ARTICLE_SCHEMA_VERSION};

    #[test]
    fn offline_reader_prefix_search_resolves_aliases_and_rejects_tampering() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-offline-reader-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();

        let page = PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 1042,
                timestamp: "2026-10-10T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Earth".into(),
            display_title: "Earth".into(),
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
        }));
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
        append_record_frame(&mut bytes, &page).unwrap();
        let redirect_offset = bytes.len() as u64;
        append_record_frame(&mut bytes, &redirect).unwrap();
        fs::write(root.join("reader.shard"), &bytes).unwrap();

        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "reader.shard", 0, redirect_offset)
            .unwrap();
        catalog
            .insert_verified(
                &root,
                "reader.shard",
                redirect_offset,
                bytes.len() as u64 - redirect_offset,
            )
            .unwrap();
        let index = OfflineSearchIndex::in_memory().unwrap();
        let reader = OfflineReader::new(&catalog, &index, &root);

        for prefix in ["earth", "The_blue", "Planet_"] {
            let hits = reader.search_title_prefix("enwiki", prefix, 10).unwrap();
            assert_eq!(hits.len(), 1, "{prefix}");
            assert_eq!(hits[0].key.page_id, 42);
            assert_eq!(hits[0].revision.revision_id, 1042);
        }
        assert!(reader
            .search_title_prefix("enwiki", "Missing", 10)
            .unwrap()
            .is_empty());
        assert!(matches!(
            reader.search_title_prefix("enwiki", "Earth", 0),
            Err(ReaderError::Catalog(CatalogError::InvalidCatalogEntry))
        ));

        bytes[12] ^= 1;
        fs::write(root.join("reader.shard"), &bytes).unwrap();
        assert!(matches!(
            reader.search_title_prefix("enwiki", "Earth", 10),
            Err(ReaderError::Catalog(CatalogError::DigestMismatch))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
