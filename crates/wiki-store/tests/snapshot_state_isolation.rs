//! STORE-003 disk-backed acceptance path: a failed activation leaves the prior
//! content and bookmarks intact across process restart, a successful switch
//! retains the previous snapshot and the separate user state, and a corrupted
//! derived search index is rebuilt from the snapshot's own catalog and shards
//! without touching canonical content, media or user state.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_store::activation::{ActiveSnapshotStore, PackMediaActivationError};
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::media_install::install_bound_owned_media;
use wiki_store::media_inventory::RequiredMedia;
use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::{MediaNotice, MediaRegistry, OwnerKind};
use wiki_store::offline_reader::VerifiedOfflineCatalogReader;
use wiki_store::record_codec::append_record_frame;
use wiki_store::search_index::OfflineSearchIndex;
use wiki_store::user_state::UserState;

type ShardInventory = BTreeMap<String, (u64, String)>;

struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn key() -> ArticleKey {
    ArticleKey {
        project: "enwiki".into(),
        page_id: 42,
    }
}

fn article(revision_id: u64, body: &str) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: key(),
        revision: Revision {
            revision_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: "Earth".into(),
        display_title: "Earth".into(),
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
        rendered_html: "<article>Earth</article>".into(),
        is_disambiguation: false,
    }
}

/// Stage `snapshots/<id>` with one canonical shard, its catalog, a persisted
/// FTS index and a manifest placeholder. Returns the authenticated inventory
/// a caller would have taken from the (separately verified) pack manifest.
fn stage_snapshot(root: &Path, id: &str, revision_id: u64, body: &str) -> ShardInventory {
    let candidate = root.join("snapshots").join(id);
    fs::create_dir_all(&candidate).unwrap();
    let mut shard = Vec::new();
    append_record_frame(
        &mut shard,
        &PageRecord::Article(Box::new(article(revision_id, body))),
    )
    .unwrap();
    fs::write(candidate.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&candidate.join("catalog.sqlite")).unwrap();
    catalog
        .insert_verified(&candidate, "articles.shard", 0, shard.len() as u64)
        .unwrap();
    OfflineSearchIndex::replace_persisted_from_catalog(
        &candidate.join("search.sqlite"),
        &catalog,
        &candidate,
    )
    .unwrap();
    fs::write(candidate.join("manifest.json"), b"{}").unwrap();
    BTreeMap::from([(
        "articles.shard".to_owned(),
        (shard.len() as u64, format!("{:x}", Sha256::digest(&shard))),
    )])
}

fn install_pack_image(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    id: &str,
) -> Vec<RequiredMedia> {
    let image = format!("required diagram for {id}").into_bytes();
    let notice = MediaNotice {
        mime: "image/png".into(),
        source_url: "https://commons.wikimedia.org/wiki/File:Earth.png".into(),
        creator: "Author".into(),
        license: "CC BY-SA 4.0".into(),
    };
    let digest =
        install_bound_owned_media(store, registry, &image, &notice, OwnerKind::Pack, id).unwrap();
    vec![RequiredMedia {
        digest,
        bytes: image.len() as u64,
    }]
}

fn open_snapshot(
    root: &Path,
    id: &str,
    inventory: &ShardInventory,
) -> VerifiedOfflineCatalogReader {
    let snapshot = root.join("snapshots").join(id);
    VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &snapshot.join("catalog.sqlite"),
        &snapshot.join("search.sqlite"),
        &snapshot,
        inventory,
    )
    .unwrap()
}

fn open_revision(root: &Path, id: &str, inventory: &ShardInventory) -> u64 {
    open_snapshot(root, id, inventory)
        .reader()
        .open_page(&key())
        .unwrap()
        .unwrap()
        .revision
        .revision_id
}

fn digest_file(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

fn assert_user_state_intact(db: &Path) {
    // A fresh connection stands in for a restarted process.
    let user = UserState::open(db).unwrap();
    assert_eq!(
        user.bookmark(&key()).unwrap(),
        Some((1001, "saved place".into()))
    );
    assert_eq!(user.reading_position(&key()).unwrap(), Some((1001, 2048)));
    assert_eq!(
        user.setting("reader.theme").unwrap().as_deref(),
        Some("dark")
    );
}

#[test]
fn failed_activation_switch_and_index_rebuild_preserve_content_and_bookmarks() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let guard = TestRoot(std::env::temp_dir().join(format!(
        "wiki-state-isolation-{}-{stamp}",
        std::process::id()
    )));
    let root = guard.0.clone();
    let media_root = root.join("media");
    fs::create_dir_all(&media_root).unwrap();

    let old_inventory = stage_snapshot(&root, "old", 1001, "the old planet text");
    let new_inventory = stage_snapshot(&root, "new", 1002, "the new planet text");
    let store = MediaObjectStore::new(&media_root);
    let mut registry = MediaRegistry::open(&root.join("registry.sqlite")).unwrap();
    registry.bind_media_root(&media_root).unwrap();
    let old_media = install_pack_image(&store, &mut registry, "old");
    let new_media = install_pack_image(&store, &mut registry, "new");

    ActiveSnapshotStore::new(&root)
        .switch_to_manifest_verified_with_bound_pack_media(
            "old",
            &store,
            &registry,
            &old_media,
            &old_inventory,
        )
        .unwrap();

    // User state lives beside, never inside, the immutable snapshots.
    fs::create_dir_all(root.join("user-state")).unwrap();
    let db = root.join("user-state/user.sqlite");
    {
        let user = UserState::open(&db).unwrap();
        user.set_bookmark(&key(), 1001, "saved place").unwrap();
        user.set_reading_position(&key(), 1001, 2048).unwrap();
        user.set_setting("reader.theme", "dark").unwrap();
    }
    assert!(!db.starts_with(root.join("snapshots")));

    // 1. Forced failure before activation: the new candidate's shard is
    //    corrupted after staging, so verification refuses it.
    let shard = root.join("snapshots/new/articles.shard");
    let good_shard = fs::read(&shard).unwrap();
    fs::write(&shard, b"truncated by a failed download").unwrap();
    let attempt = ActiveSnapshotStore::new(&root)
        .switch_to_manifest_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &new_media,
            &new_inventory,
        );
    assert!(matches!(attempt, Err(PackMediaActivationError::Reader(_))));
    // Restart: fresh handles see the prior content and the same bookmarks.
    assert_eq!(
        ActiveSnapshotStore::new(&root)
            .current()
            .unwrap()
            .as_deref(),
        Some("old")
    );
    assert_eq!(open_revision(&root, "old", &old_inventory), 1001);
    assert_user_state_intact(&db);

    // 2. The repaired candidate activates; the old snapshot is retained and
    //    still independently readable for rollback, and user state is shared.
    fs::write(&shard, &good_shard).unwrap();
    ActiveSnapshotStore::new(&root)
        .switch_to_manifest_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &new_media,
            &new_inventory,
        )
        .unwrap();
    assert_eq!(
        ActiveSnapshotStore::new(&root)
            .current()
            .unwrap()
            .as_deref(),
        Some("new")
    );
    assert_eq!(open_revision(&root, "new", &new_inventory), 1002);
    assert_eq!(open_revision(&root, "old", &old_inventory), 1001);
    assert_user_state_intact(&db);

    // 3. The active snapshot's derived index is corrupted. Canonical content,
    //    media and user state must not be needed again to recover search.
    let active = root.join("snapshots/new");
    let index = active.join("search.sqlite");
    let canonical = [active.join("articles.shard"), active.join("catalog.sqlite")];
    let canonical_before: Vec<_> = canonical.iter().map(|p| digest_file(p)).collect();
    let user_before = digest_file(&db);
    fs::write(&index, b"not an sqlite database").unwrap();
    assert!(VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &active.join("catalog.sqlite"),
        &index,
        &active,
        &new_inventory,
    )
    .is_err());

    let catalog = SnapshotCatalog::open_read_only(&active.join("catalog.sqlite")).unwrap();
    OfflineSearchIndex::replace_persisted_from_verified_shards(
        &index,
        &catalog,
        &active,
        &new_inventory,
    )
    .unwrap();
    drop(catalog);

    let reader = open_snapshot(&root, "new", &new_inventory);
    let hits = reader.reader().search_articles("planet", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].revision.revision_id, 1002);
    drop(reader);
    let canonical_after: Vec<_> = canonical.iter().map(|p| digest_file(p)).collect();
    assert_eq!(canonical_before, canonical_after);
    assert_eq!(digest_file(&db), user_before);
    for media in old_media.iter().chain(&new_media) {
        assert_eq!(
            store.read_verified(&media.digest).unwrap().len() as u64,
            media.bytes
        );
    }
    assert_eq!(
        ActiveSnapshotStore::new(&root)
            .current()
            .unwrap()
            .as_deref(),
        Some("new")
    );
    assert_user_state_intact(&db);
    drop(registry);
}
