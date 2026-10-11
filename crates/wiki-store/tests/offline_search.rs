use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Redirect, Revision,
    ARTICLE_SCHEMA_VERSION,
};
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::record_codec::append_record_frame;
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

#[test]
fn streaming_source_failure_rolls_back_and_success_replaces_revision() {
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index.rebuild(&[article(1, "Physics", "gravity")]).unwrap();
    let interrupted = vec![
        Ok(article(2, "Chemistry", "molecules")),
        Err(SearchError::InvalidIdentity),
    ];
    assert!(matches!(
        index.rebuild_streaming(interrupted),
        Err(SearchError::InvalidIdentity)
    ));
    assert_eq!(index.search("gravity", 5).unwrap().len(), 1);
    assert!(index.search("molecules", 5).unwrap().is_empty());
    index
        .rebuild_streaming(vec![Ok(article(2, "Chemistry", "molecules"))])
        .unwrap();
    assert!(index.search("gravity", 5).unwrap().is_empty());
    assert_eq!(index.search("molecules", 5).unwrap()[0].key.page_id, 2);
}

#[test]
fn catalog_rebuild_skips_redirects_and_rolls_back_on_late_corruption() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-catalog-fts-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let records = [
        PageRecord::Article(Box::new(article(1, "Physics", "gravity"))),
        PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 10,
            },
            title: "Physics alias".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 1,
            },
        }),
        PageRecord::Article(Box::new(article(2, "Chemistry", "molecules"))),
    ];
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for record in &records {
        let offset = shard.len() as u64;
        append_record_frame(&mut shard, record).unwrap();
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(root.join("records.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    for (offset, bytes) in &frames {
        catalog
            .insert_verified(&root, "records.shard", *offset, *bytes)
            .unwrap();
    }
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index
        .rebuild(&[article(99, "Previous", "retained")])
        .unwrap();
    index.rebuild_from_catalog(&catalog, &root).unwrap();
    assert!(index.search("retained", 10).unwrap().is_empty());
    let gravity_hit = index.search("gravity", 10).unwrap().remove(0);
    assert_eq!(gravity_hit.key.page_id, 1);
    assert_eq!(
        index
            .resolve_hit(&catalog, &root, &gravity_hit)
            .unwrap()
            .title,
        "Physics"
    );
    assert_eq!(index.search("molecules", 10).unwrap()[0].key.page_id, 2);
    let mut stale = gravity_hit.clone();
    stale.revision_id += 1;
    assert!(matches!(
        index.resolve_hit(&catalog, &root, &stale),
        Err(SearchError::StaleIndex)
    ));
    let alias_hits = index.search("alias", 10).unwrap();
    assert_eq!(alias_hits.len(), 1);
    assert_eq!(alias_hits[0].key.page_id, 1);
    assert_eq!(alias_hits[0].revision_id, 42);
    assert_eq!(
        index.resolve_hit(&catalog, &root, &alias_hits[0]).unwrap().title,
        "Physics"
    );

    // The first article is valid but the last frame is corrupt: the old
    // complete index must remain visible after the attempted replacement.
    let last = frames[2].0 as usize;
    shard[last + 12] ^= 1;
    fs::write(root.join("records.shard"), &shard).unwrap();
    assert!(matches!(
        index.rebuild_from_catalog(&catalog, &root),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(index.search("gravity", 10).unwrap()[0].key.page_id, 1);
    assert_eq!(index.search("molecules", 10).unwrap()[0].key.page_id, 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_search_matches_display_title_and_unicode_aliases() {
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    let mut earth = article(42, "Earth", "terrestrial planet");
    earth.display_title = "Planet Earth".into();
    earth.aliases = vec!["The Blue Planet".into(), "地球".into()];
    index.rebuild(&[earth]).unwrap();
    for query in ["blue", "地球", "planet"] {
        let hits = index.search(query, 5).unwrap();
        assert_eq!(hits.len(), 1, "missing title variant: {query}");
        assert_eq!(hits[0].key.page_id, 42);
        assert_eq!(hits[0].revision_id, 42);
    }
}


#[test]
fn redirect_chains_unicode_and_single_canonical_hit() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-redirect-fts-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let records = [
        PageRecord::Article(Box::new(article(1, "Earth", "terrestrial sphere"))),
        PageRecord::Redirect(Redirect {
            from: ArticleKey { project: "enwiki".into(), page_id: 10 },
            title: "地球".into(),
            to: ArticleKey { project: "enwiki".into(), page_id: 11 },
        }),
        PageRecord::Redirect(Redirect {
            from: ArticleKey { project: "enwiki".into(), page_id: 11 },
            title: "The Blue Planet".into(),
            to: ArticleKey { project: "enwiki".into(), page_id: 1 },
        }),
    ];
    let mut bytes = Vec::new();
    let mut frames = Vec::new();
    for record in &records {
        let start = bytes.len() as u64;
        append_record_frame(&mut bytes, record).unwrap();
        frames.push((start, bytes.len() as u64 - start));
    }
    fs::write(root.join("redirects.shard"), &bytes).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    for (offset, length) in frames {
        catalog.insert_verified(&root, "redirects.shard", offset, length).unwrap();
    }
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index.rebuild_from_catalog(&catalog, &root).unwrap();
    for query in ["地球", "blue", "sphere"] {
        let hits = index.search(query, 5).unwrap();
        assert_eq!(hits.len(), 1, "unexpected hit count for {query}");
        assert_eq!(hits[0].key.page_id, 1);
        assert_eq!(hits[0].revision_id, 42);
        assert_eq!(index.resolve_hit(&catalog, &root, &hits[0]).unwrap().title, "Earth");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bad_redirect_chain_preserves_last_good_index() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-redirect-fail-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut bytes = Vec::new();
    let redirect = PageRecord::Redirect(Redirect {
        from: ArticleKey { project: "enwiki".into(), page_id: 10 },
        title: "Lost title".into(),
        to: ArticleKey { project: "enwiki".into(), page_id: 999 },
    });
    append_record_frame(&mut bytes, &PageRecord::Article(Box::new(article(1, "Earth", "new text")))).unwrap();
    let next = bytes.len() as u64;
    append_record_frame(&mut bytes, &redirect).unwrap();
    fs::write(root.join("broken.shard"), &bytes).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    catalog.insert_verified(&root, "broken.shard", 0, next).unwrap();
    catalog.insert_verified(&root, "broken.shard", next, bytes.len() as u64 - next).unwrap();
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index.rebuild(&[article(2, "Previous", "retained")]).unwrap();
    assert!(matches!(
        index.rebuild_from_catalog(&catalog, &root),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(index.search("retained", 5).unwrap()[0].key.page_id, 2);
    assert!(index.search("new", 5).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}
