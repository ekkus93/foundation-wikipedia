//! Exact-revision Wikidata navigation over verified installed articles.
//!
//! Wikidata item identity is not a revision identity: citation navigation must
//! reject an installed article whose revision differs from the saved target.
//! The existing offline reader verifies catalog frames and never uses HTTP.

use wiki_model::Article;

use crate::offline_reader::{OfflineReader, ReaderError};

impl OfflineReader<'_> {
    /// Return verified articles for one Wikidata item only if every installed
    /// match is at the caller's exact revision. Do not silently relocate an
    /// old citation onto newer content, even when page identity is unchanged.
    pub fn open_exact_wikidata_revision(
        &self,
        project: &str,
        item: &str,
        expected: u64,
        limit: usize,
    ) -> Result<Vec<Article>, ReaderError> {
        self.open_wikidata(project, item, limit)?
            .into_iter()
            .map(|article| {
                let actual = article.revision.revision_id;
                if actual != expected {
                    Err(ReaderError::StaleRevision { expected, actual })
                } else {
                    Ok(article)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogError, SnapshotCatalog};
    use crate::record_codec::append_record_frame;
    use crate::search_index::OfflineSearchIndex;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiki_model::{ArticleKey, PageRecord, Revision, ARTICLE_SCHEMA_VERSION};

    #[test]
    fn wikidata_revision_navigation_rejects_stale_or_corrupted_frames() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-exact-wikidata-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let article = PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 1042,
                timestamp: "2026-10-22T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Earth".into(),
            display_title: "Earth".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: Some("Q2".into()),
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Earth</article>".into(),
            is_disambiguation: false,
        }));
        let mut bytes = Vec::new();
        append_record_frame(&mut bytes, &article).unwrap();
        fs::write(root.join("article.shard"), &bytes).unwrap();
        let mut catalog = SnapshotCatalog::in_memory().unwrap();
        catalog
            .insert_verified(&root, "article.shard", 0, bytes.len() as u64)
            .unwrap();
        let index = OfflineSearchIndex::in_memory().unwrap();
        let reader = OfflineReader::new(&catalog, &index, &root);

        let hits = reader
            .open_exact_wikidata_revision("enwiki", "Q2", 1042, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key.page_id, 42);
        assert!(reader
            .open_exact_wikidata_revision("frwiki", "Q2", 1042, 10)
            .unwrap()
            .is_empty());
        assert!(matches!(
            reader.open_exact_wikidata_revision("enwiki", "Q2", 1043, 10),
            Err(ReaderError::StaleRevision {
                expected: 1043,
                actual: 1042
            })
        ));
        assert!(matches!(
            reader.open_exact_wikidata_revision("enwiki", "Q0", 1042, 10),
            Err(ReaderError::Catalog(CatalogError::InvalidCatalogEntry))
        ));

        bytes[12] ^= 1;
        fs::write(root.join("article.shard"), bytes).unwrap();
        assert!(matches!(
            reader.open_exact_wikidata_revision("enwiki", "Q2", 1042, 10),
            Err(ReaderError::Catalog(CatalogError::DigestMismatch))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
