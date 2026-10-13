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
fn canonical_title_outranks_repeated_body_mentions() {
    // FTS5 weights are positional across all five columns. The first three
    // are UNINDEXED metadata; the fourth is the canonical title, not the first.
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index
        .rebuild(&[
            article(1, "Physics", "Matter and energy"),
            article(2, "Mathematics", &"Physics ".repeat(10)),
            article(3, "Geometry", "Physics studies matter"),
        ])
        .unwrap();
    let hits = index.search("Physics", 10).unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.key.page_id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
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
    let verified = index
        .search_verified(&catalog, &root, "gravity molecules", 10)
        .unwrap();
    assert_eq!(verified.len(), 2);
    let mut verified_ids: Vec<_> = verified.iter().map(|a| a.key.page_id).collect();
    verified_ids.sort_unstable();
    assert_eq!(verified_ids, vec![1, 2]);
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
        index
            .resolve_hit(&catalog, &root, &alias_hits[0])
            .unwrap()
            .title,
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
    // FTS may still contain the prior entries when one immutable frame
    // corrupts. Never display an apparently complete partial search result.
    assert!(matches!(
        index.search_verified(&catalog, &root, "gravity molecules", 10),
        Err(SearchError::Catalog(_))
    ));
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
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-redirect-fts-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let records = [
        PageRecord::Article(Box::new(article(1, "Earth", "terrestrial sphere"))),
        PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 10,
            },
            title: "地球".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 11,
            },
        }),
        PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 11,
            },
            title: "The Blue Planet".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 1,
            },
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
        catalog
            .insert_verified(&root, "redirects.shard", offset, length)
            .unwrap();
    }
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index.rebuild_from_catalog(&catalog, &root).unwrap();
    for query in ["地球", "blue", "sphere"] {
        let hits = index.search(query, 5).unwrap();
        assert_eq!(hits.len(), 1, "unexpected hit count for {query}");
        assert_eq!(hits[0].key.page_id, 1);
        assert_eq!(hits[0].revision_id, 42);
        assert_eq!(
            index.resolve_hit(&catalog, &root, &hits[0]).unwrap().title,
            "Earth"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bad_redirect_chain_preserves_last_good_index() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-redirect-fail-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut bytes = Vec::new();
    let redirect = PageRecord::Redirect(Redirect {
        from: ArticleKey {
            project: "enwiki".into(),
            page_id: 10,
        },
        title: "Lost title".into(),
        to: ArticleKey {
            project: "enwiki".into(),
            page_id: 999,
        },
    });
    append_record_frame(
        &mut bytes,
        &PageRecord::Article(Box::new(article(1, "Earth", "new text"))),
    )
    .unwrap();
    let next = bytes.len() as u64;
    append_record_frame(&mut bytes, &redirect).unwrap();
    fs::write(root.join("broken.shard"), &bytes).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    catalog
        .insert_verified(&root, "broken.shard", 0, next)
        .unwrap();
    catalog
        .insert_verified(&root, "broken.shard", next, bytes.len() as u64 - next)
        .unwrap();
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index
        .rebuild(&[article(2, "Previous", "retained")])
        .unwrap();
    assert!(matches!(
        index.rebuild_from_catalog(&catalog, &root),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(index.search("retained", 5).unwrap()[0].key.page_id, 2);
    assert!(index.search("new", 5).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cyclic_redirects_cannot_replace_previously_published_search_index() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "wiki-redirect-cycle-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let records = [
        PageRecord::Article(Box::new(article(1, "Earth", "not yet published"))),
        PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 10,
            },
            title: "Loop A".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 11,
            },
        }),
        PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id: 11,
            },
            title: "Loop B".into(),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 10,
            },
        }),
    ];
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for record in &records {
        let start = bytes.len() as u64;
        append_record_frame(&mut bytes, record).unwrap();
        offsets.push((start, bytes.len() as u64 - start));
    }
    fs::write(root.join("cycle.shard"), &bytes).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    for (start, frame_bytes) in offsets {
        catalog
            .insert_verified(&root, "cycle.shard", start, frame_bytes)
            .unwrap();
    }
    let mut index = OfflineSearchIndex::in_memory().unwrap();
    index
        .rebuild(&[article(2, "Previous", "old snapshot")])
        .unwrap();
    assert!(matches!(
        index.rebuild_from_catalog(&catalog, &root),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(index.search("snapshot", 10).unwrap()[0].key.page_id, 2);
    assert!(index.search("Loop", 10).unwrap().is_empty());
    assert!(index.search("published", 10).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_persisted_fts_is_rebuilt_without_touching_source_or_media() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-recover-fts-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let record = PageRecord::Article(Box::new(article(1, "Earth", "gravity")));
    let mut bytes = Vec::new();
    append_record_frame(&mut bytes, &record).unwrap();
    fs::write(root.join("data.shard"), &bytes).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    catalog
        .insert_verified(&root, "data.shard", 0, bytes.len() as u64)
        .unwrap();
    let index_path = root.join("derived.sqlite");
    fs::write(&index_path, b"corrupt SQLite bytes").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&index_path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    OfflineSearchIndex::replace_persisted_from_catalog(&index_path, &catalog, &root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The atomically published file must not inherit the normal 0644
        // permissions of a default create-new file on common CI systems.
        assert_eq!(
            fs::metadata(&index_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let index = OfflineSearchIndex::open(&index_path).unwrap();
    let hits = index.search("gravity", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].key.page_id, 1);
    assert_eq!(fs::read(root.join("data.shard")).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".offline-fts."))
            .count(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_persisted_rebuild_leaves_previously_searchable_file_unchanged() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "wiki-recover-rollback-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let index_path = root.join("derived.sqlite");
    {
        let mut index = OfflineSearchIndex::open(&index_path).unwrap();
        index
            .rebuild(&[article(3, "Previous", "retained snapshot")])
            .unwrap();
    }
    let before = fs::read(&index_path).unwrap();
    let redirect = PageRecord::Redirect(Redirect {
        from: ArticleKey {
            project: "enwiki".into(),
            page_id: 10,
        },
        title: "Missing target".into(),
        to: ArticleKey {
            project: "enwiki".into(),
            page_id: 999,
        },
    });
    let mut bytes = Vec::new();
    append_record_frame(&mut bytes, &redirect).unwrap();
    fs::write(root.join("bad.shard"), &bytes).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    catalog
        .insert_verified(&root, "bad.shard", 0, bytes.len() as u64)
        .unwrap();
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_catalog(&index_path, &catalog, &root),
        Err(SearchError::Catalog(_))
    ));
    assert_eq!(fs::read(&index_path).unwrap(), before);
    assert_eq!(
        OfflineSearchIndex::open(&index_path)
            .unwrap()
            .search("retained", 10)
            .unwrap()[0]
            .key
            .page_id,
        3
    );
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".offline-fts."))
            .count(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn persisted_rebuild_rejects_symlinked_database_or_parent_directory() {
    use std::os::unix::fs::symlink;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-fts-symlink-{}-{stamp}", std::process::id()));
    let real = root.join("real");
    fs::create_dir_all(&real).unwrap();
    let outside = real.join("outside.db");
    fs::write(&outside, b"protected data").unwrap();
    let catalog = SnapshotCatalog::in_memory().unwrap();

    let linked_file = root.join("index.sqlite");
    symlink(&outside, &linked_file).unwrap();
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_catalog(&linked_file, &catalog, &real),
        Err(SearchError::UnsafePath)
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"protected data");

    let linked_parent = root.join("linked-parent");
    symlink(&real, &linked_parent).unwrap();
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_catalog(
            &linked_parent.join("index.sqlite"),
            &catalog,
            &real
        ),
        Err(SearchError::UnsafePath)
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"protected data");
    assert_eq!(fs::read_dir(&real).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn persisted_index_open_and_rebuild_reject_unsafe_permissions_links_and_ancestors() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-fts-private-{}-{stamp}", std::process::id()));
    let real = root.join("real");
    let nested = real.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let index_path = nested.join("search.sqlite");
    fs::write(&index_path, b"existing search index").unwrap();
    let catalog = SnapshotCatalog::in_memory().unwrap();

    // Neither SQLite opening nor atomic replacement may touch a database
    // whose contents have already been exposed by the filesystem.
    fs::set_permissions(&index_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        OfflineSearchIndex::open(&index_path),
        Err(SearchError::UnsafePath)
    ));
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_catalog(&index_path, &catalog, &real),
        Err(SearchError::UnsafePath)
    ));
    fs::set_permissions(&index_path, fs::Permissions::from_mode(0o600)).unwrap();

    let second_link = nested.join("another-name.sqlite");
    fs::hard_link(&index_path, &second_link).unwrap();
    assert!(matches!(
        OfflineSearchIndex::open(&index_path),
        Err(SearchError::UnsafePath)
    ));
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_catalog(&index_path, &catalog, &real),
        Err(SearchError::UnsafePath)
    ));
    fs::remove_file(&second_link).unwrap();

    // The direct parent is a real directory; a higher ancestor is a
    // symlink, and must be rejected before any SQLite sidecar is created.
    let alias = root.join("linked-grandparent");
    symlink(&real, &alias).unwrap();
    let via_alias = alias.join("nested").join("search.sqlite");
    assert!(matches!(
        OfflineSearchIndex::open(&via_alias),
        Err(SearchError::UnsafePath)
    ));
    assert!(matches!(
        OfflineSearchIndex::replace_persisted_from_catalog(&via_alias, &catalog, &real),
        Err(SearchError::UnsafePath)
    ));
    assert_eq!(fs::read(&index_path).unwrap(), b"existing search index");
    fs::remove_dir_all(&root).unwrap();
}
