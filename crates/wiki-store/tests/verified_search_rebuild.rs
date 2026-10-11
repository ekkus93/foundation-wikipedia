use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::record_codec::append_record_frame;
use wiki_store::search_index::{OfflineSearchIndex, SearchError};

fn article(id: u64, title: &str, body: &str) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: id,
        },
        revision: Revision {
            revision_id: id + 100,
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
        rendered_html: "<article>verified fixture</article>".into(),
        is_disambiguation: false,
    }
}

#[test]
fn verified_shard_rebuild_refuses_unindexed_tail_and_bad_manifest_hash() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-fts-verified-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let index_path = root.join("search.sqlite");
    {
        let mut index = OfflineSearchIndex::open(&index_path).unwrap();
        index
            .rebuild(&[article(99, "Retained", "old snapshot")])
            .unwrap();
    }
    let original = fs::read(&index_path).unwrap();

    let mut shard = Vec::new();
    let first = PageRecord::Article(Box::new(article(1, "Physics", "gravity")));
    append_record_frame(&mut shard, &first).unwrap();
    let first_len = shard.len() as u64;
    let second = PageRecord::Article(Box::new(article(2, "Chemistry", "molecules")));
    append_record_frame(&mut shard, &second).unwrap();
    fs::write(root.join("articles.shard"), &shard).unwrap();

    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    catalog
        .insert_verified(&root, "articles.shard", 0, first_len)
        .unwrap();

    let mut expected = BTreeMap::new();
    expected.insert(
        "articles.shard".into(),
        (shard.len() as u64, format!("{:x}", Sha256::digest(&shard))),
    );
    // A digest-verified indexed frame is insufficient if the shard also
    // contains an unindexed article. Do not publish a partial search index.
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_verified_shards(
            &index_path,
            &catalog,
            &root,
            &expected
        ),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(fs::read(&index_path).unwrap(), original);

    catalog
        .insert_verified(
            &root,
            "articles.shard",
            first_len,
            shard.len() as u64 - first_len,
        )
        .unwrap();

    let mut incorrect = expected.clone();
    incorrect.get_mut("articles.shard").unwrap().1 = "0".repeat(64);
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_verified_shards(
            &index_path,
            &catalog,
            &root,
            &incorrect
        ),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(fs::read(&index_path).unwrap(), original);

    // The complete catalog and independently supplied matching digest now
    // permit atomic publication of both searchable articles.
    OfflineSearchIndex::replace_persisted_from_verified_shards(
        &index_path,
        &catalog,
        &root,
        &expected,
    )
    .unwrap();
    let index = OfflineSearchIndex::open(&index_path).unwrap();
    assert!(index.search("snapshot", 10).unwrap().is_empty());
    assert_eq!(index.search("gravity", 10).unwrap()[0].key.page_id, 1);
    assert_eq!(index.search("molecules", 10).unwrap()[0].key.page_id, 2);
    assert_eq!(fs::read(root.join("articles.shard")).unwrap(), shard);
    fs::remove_dir_all(root).unwrap();
}
