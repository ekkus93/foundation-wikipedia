//! Read-only offline article navigation over one caller-verified snapshot.
//!
//! This service uses no network APIs. A missing page returns `None`, while
//! corrupt frames and dangling redirects return errors, never a stale article.
//! Source-publisher authentication and pack activation are caller obligations.

use std::collections::HashSet;
use std::path::Path;

use wiki_model::{Article, ArticleKey, PageRecord};

use crate::catalog::{CatalogError, SnapshotCatalog};
use crate::search_index::{OfflineSearchIndex, SearchError};

#[derive(Debug)]
pub enum ReaderError {
    Catalog(CatalogError),
    Search(SearchError),
    StaleRevision { expected: u64, actual: u64 },
    PageIdentityMismatch { expected: u64, actual: u64 },
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

    /// Construct a reader only after auditing every installed article
    /// against the derived FTS identity inventory, including other editions.
    /// Callers must separately authenticate the immutable snapshot, verify
    /// mandatory media, and keep the catalog and shards immutable.
    pub fn new_verified(
        catalog: &'a SnapshotCatalog,
        index: &'a OfflineSearchIndex,
        root: &'a Path,
    ) -> Result<Self, ReaderError> {
        index.verify_catalog_membership(catalog, root)?;
        Ok(Self::new(catalog, index, root))
    }

    /// Open an installed page by stable project/page ID, including redirects.
    /// Only a genuinely absent key reports `None`.
    pub fn open_page(&self, key: &ArticleKey) -> Result<Option<Article>, ReaderError> {
        // A redirect has a stable target page ID. Resolving its mutable title
        // again would incorrectly depend on a separate title-index entry.
        // Every hop must still pass the catalog's digest and identity checks.
        let mut current = key.clone();
        let mut visited = HashSet::new();
        for _ in 0..32 {
            if !visited.insert(current.clone()) {
                return Err(CatalogError::RedirectCycle.into());
            }
            match self.catalog.read_record(self.root, &current)? {
                None if current == *key => return Ok(None),
                None => return Err(CatalogError::DanglingRedirect.into()),
                Some(PageRecord::Article(article)) => return Ok(Some(*article)),
                Some(PageRecord::Redirect(redirect)) => current = redirect.to,
            }
        }
        Err(CatalogError::RedirectTooDeep.into())
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
            .map(|article| Self::check_revision(article, expected))
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

    /// Open an installed canonical title, alias or redirect only at the
    /// caller's exact revision. A newer installed revision cannot silently
    /// satisfy a historical citation or saved navigation target.
    pub fn open_exact_title_revision(
        &self,
        project: &str,
        title: &str,
        expected: u64,
    ) -> Result<Option<Article>, ReaderError> {
        self.open_title(project, title)?
            .map(|article| Self::check_revision(article, expected))
            .transpose()
    }

    /// Resolve a saved title/alias/redirect only when both the stable page ID
    /// and exact revision agree with the caller's recorded identity. A title
    /// reassigned to a different page must never satisfy an old citation,
    /// even if the two articles happen to share a revision number.
    pub fn open_exact_title_page_revision(
        &self,
        project: &str,
        title: &str,
        expected_page_id: u64,
        expected_revision: u64,
    ) -> Result<Option<Article>, ReaderError> {
        if expected_page_id == 0 || expected_page_id > i64::MAX as u64 {
            return Err(CatalogError::InvalidCatalogEntry.into());
        }
        self.open_title(project, title)?
            .map(|article| {
                if article.key.page_id != expected_page_id {
                    return Err(ReaderError::PageIdentityMismatch {
                        expected: expected_page_id,
                        actual: article.key.page_id,
                    });
                }
                Self::check_revision(article, expected_revision)
            })
            .transpose()
    }

    fn check_revision(article: Article, expected: u64) -> Result<Article, ReaderError> {
        let actual = article.revision.revision_id;
        if actual != expected {
            Err(ReaderError::StaleRevision { expected, actual })
        } else {
            Ok(article)
        }
    }

    /// Search installed articles, verifying all matched immutable frames
    /// before returning any article body to the caller.
    pub fn search_articles(&self, query: &str, limit: usize) -> Result<Vec<Article>, ReaderError> {
        Ok(self
            .index
            .search_verified(self.catalog, self.root, query, limit)?)
    }

    /// Query installed FTS entries for one edition only; all returned pages
    /// are rechecked against their immutable catalog frames and revisions.
    pub fn search_project_articles(
        &self,
        project: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Article>, ReaderError> {
        Ok(self
            .index
            .search_verified_project(self.catalog, self.root, project, query, limit)?)
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

    /// Return bounded, verified local articles matching a Wikidata item.
    pub fn open_wikidata(
        &self,
        project: &str,
        item: &str,
        limit: usize,
    ) -> Result<Vec<Article>, ReaderError> {
        self.catalog
            .lookup_wikidata_verified(self.root, project, item, limit)
            .map_err(ReaderError::from)
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

        let database = root.join("catalog.sqlite");
        let mut catalog = SnapshotCatalog::open(&database).unwrap();
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
        let mut index = OfflineSearchIndex::in_memory().unwrap();
        index.rebuild_from_catalog(&catalog, &root).unwrap();
        let reader = OfflineReader::new_verified(&catalog, &index, &root).unwrap();
        let empty_index = OfflineSearchIndex::in_memory().unwrap();
        assert!(matches!(
            OfflineReader::new_verified(&catalog, &empty_index, &root),
            Err(ReaderError::Search(SearchError::StaleIndex))
        ));
        let scoped = reader
            .search_project_articles("enwiki", "Earth", 10)
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].key.page_id, 42);
        assert_eq!(scoped[0].revision.revision_id, 1042);
        assert!(reader
            .search_project_articles("frwiki", "Earth", 10)
            .unwrap()
            .is_empty());

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

        for title in ["Earth", "The_blue_planet", "Planet Earth"] {
            let article = reader
                .open_exact_title_revision("enwiki", title, 1042)
                .unwrap()
                .unwrap();
            assert_eq!(article.key.page_id, 42);
            assert_eq!(article.revision.revision_id, 1042);
        }
        assert!(reader
            .open_exact_title_revision("enwiki", "Not installed", 1042)
            .unwrap()
            .is_none());
        for title in ["Earth", "The_blue_planet", "Planet Earth"] {
            assert_eq!(
                reader
                    .open_exact_title_page_revision("enwiki", title, 42, 1042)
                    .unwrap()
                    .unwrap()
                    .key
                    .page_id,
                42
            );
        }
        assert!(matches!(
            reader.open_exact_title_page_revision("enwiki", "Planet Earth", 43, 1042),
            Err(ReaderError::PageIdentityMismatch {
                expected: 43,
                actual: 42
            })
        ));
        assert!(matches!(
            reader.open_exact_title_page_revision("enwiki", "Earth", 42, 999),
            Err(ReaderError::StaleRevision {
                expected: 999,
                actual: 1042
            })
        ));
        assert!(reader
            .open_exact_title_page_revision("enwiki", "Not installed", 42, 1042)
            .unwrap()
            .is_none());
        assert!(matches!(
            reader.open_exact_title_page_revision("enwiki", "Earth", 0, 1042),
            Err(ReaderError::Catalog(CatalogError::InvalidCatalogEntry))
        ));
        assert!(matches!(
            reader.open_exact_title_page_revision("enwiki", "Earth", u64::MAX, 1042),
            Err(ReaderError::Catalog(CatalogError::InvalidCatalogEntry))
        ));
        assert!(matches!(
            reader.open_exact_title_revision("enwiki", "Planet Earth", 999),
            Err(ReaderError::StaleRevision {
                expected: 999,
                actual: 1042
            })
        ));

        // A page-ID redirect must not require a title-index entry. Removing
        // that secondary row models a stale derived index independently of
        // the already verified immutable redirect frame.
        let redirect_key = ArticleKey {
            project: "enwiki".into(),
            page_id: 43,
        };
        assert_eq!(
            reader
                .open_page(&redirect_key)
                .unwrap()
                .unwrap()
                .key
                .page_id,
            42
        );
        let conn = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            conn.execute(
                "DELETE FROM title_index WHERE project='enwiki' AND page_id=43",
                [],
            )
            .unwrap(),
            1
        );
        drop(conn);
        assert!(catalog
            .lookup_key("enwiki", "Planet Earth")
            .unwrap()
            .is_none());
        assert_eq!(
            reader
                .open_page(&redirect_key)
                .unwrap()
                .unwrap()
                .key
                .page_id,
            42
        );
        assert_eq!(
            reader
                .open_exact_revision(&redirect_key, 1042)
                .unwrap()
                .unwrap()
                .key
                .page_id,
            42
        );

        bytes[12] ^= 1;
        fs::write(root.join("reader.shard"), &bytes).unwrap();
        assert!(matches!(
            OfflineReader::new_verified(&catalog, &index, &root),
            Err(ReaderError::Search(SearchError::Catalog(
                CatalogError::DigestMismatch
            )))
        ));
        assert!(matches!(
            reader.search_title_prefix("enwiki", "Earth", 10),
            Err(ReaderError::Catalog(CatalogError::DigestMismatch))
        ));
        assert!(matches!(
            reader.search_project_articles("enwiki", "Earth", 10),
            Err(ReaderError::Search(SearchError::Catalog(
                CatalogError::DigestMismatch
            )))
        ));
        assert!(matches!(
            reader.open_page(&redirect_key),
            Err(ReaderError::Catalog(CatalogError::DigestMismatch))
        ));
        assert!(matches!(
            reader.open_exact_title_page_revision("enwiki", "Earth", 42, 1042),
            Err(ReaderError::Catalog(CatalogError::DigestMismatch))
        ));
        drop(catalog);
        fs::remove_dir_all(root).unwrap();
    }
}
