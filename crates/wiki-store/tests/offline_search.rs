use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_model::{Article, ArticleKey, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};
use wiki_store::search_index::{OfflineSearchIndex, SearchError};

fn article(page_id: u64, title: &str, body: &str) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 42,
            timestamp: "2026-10-11T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: title.into(),
        display_title: title.into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph(body.into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>fixture</article>".into(),
        is_disambiguation: false,
    }
}

#[test]
fn offline_title_and_body_ranking_unicode_and_query_bounds() {
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index
        .rebuild(&[
            article(1, "Physics", "Matter and energy"),
            article(2, "Mathematics", "Physics equations and geometry"),
            article(3, "東京", "日本語 の 百科事典"),
        ])
        .unwrap();
    let hits = index.search("Physics", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].key.page_id, 1);
    assert_eq!(index.search("百科事典", 10).unwrap()[0].key.page_id, 3);
    assert!(index.search("physics", 0).unwrap().is_empty());
    assert!(index.search(&"x".repeat(8193), 10).unwrap().is_empty());
    assert!(index.search("physics NOT unknown", 10).is_ok());
}

#[test]
fn rebuild_failure_rolls_back_and_new_revision_replaces_old() {
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index.rebuild(&[article(1, "Physics", "gravity")]).unwrap();
    assert!(matches!(
        index.rebuild(&[
            article(1, "Other", "new content"),
            article(1, "Other", "duplicate"),
        ]),
        Err(SearchError::Duplicate(_))
    ));
    assert_eq!(index.search("gravity", 5).unwrap().len(), 1);
    assert!(index.search("new", 5).unwrap().is_empty());
    let mut newer = article(1, "Physics", "relativity");
    newer.revision.revision_id = 43;
    index.rebuild(&[newer]).unwrap();
    assert!(index.search("gravity", 5).unwrap().is_empty());
    assert_eq!(index.search("relativity", 5).unwrap()[0].revision_id, 43);
}

#[test]
fn disk_backed_index_survives_reopen_and_can_be_rebuilt() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-fts-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let path = root.join("derived.sqlite");
    {
        let mut index = OfflineSearchIndex::open(&path).unwrap();
        index
            .rebuild(&[article(5, "Geometry", "Euclidean triangles")])
            .unwrap();
    }
    {
        let mut index = OfflineSearchIndex::open(&path).unwrap();
        assert_eq!(index.search("triangles", 1).unwrap()[0].key.page_id, 5);
        index.rebuild(&[]).unwrap();
        assert!(index.search("triangles", 1).unwrap().is_empty());
    }
    fs::remove_dir_all(root).unwrap();
}
