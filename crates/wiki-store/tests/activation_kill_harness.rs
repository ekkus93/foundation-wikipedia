//! STORE-003 reproducible process-interruption harness (Unix).
//!
//! The test binary re-executes itself as a child that alternates snapshot
//! activations while committing user-state writes, and the parent SIGKILLs
//! it at staggered points. After every kill, fresh handles must find a valid
//! selector naming a fully readable snapshot, user state that reopens with no
//! lost committed write, and leftover private files that never block the
//! next activation.
//!
//! This exercises abrupt process death only. A killed process cannot lose
//! data the kernel already accepted, so this is NOT power-loss evidence.
#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_store::activation::ActiveSnapshotStore;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::offline_reader::VerifiedOfflineCatalogReader;
use wiki_store::record_codec::append_record_frame;
use wiki_store::search_index::OfflineSearchIndex;
use wiki_store::user_state::UserState;

const ROOT_ENV: &str = "WIKI_STORE_KILL_HARNESS_ROOT";
const READY_ENV: &str = "WIKI_STORE_KILL_HARNESS_READY";
const KILLS: u64 = 24;

type ShardInventory = BTreeMap<String, (u64, String)>;

fn key() -> ArticleKey {
    ArticleKey {
        project: "enwiki".into(),
        page_id: 7,
    }
}

fn revision_for(id: &str) -> u64 {
    if id == "old" {
        2001
    } else {
        2002
    }
}

fn stage_snapshot(root: &Path, id: &str) -> ShardInventory {
    let candidate = root.join("snapshots").join(id);
    fs::create_dir_all(&candidate).unwrap();
    let article = Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: key(),
        revision: Revision {
            revision_id: revision_for(id),
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "b".repeat(64),
        },
        title: "Moon".into(),
        display_title: "Moon".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph(format!("{id} lunar text")),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>Moon</article>".into(),
        is_disambiguation: false,
    };
    let mut shard = Vec::new();
    append_record_frame(&mut shard, &PageRecord::Article(Box::new(article))).unwrap();
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

fn user_db(root: &Path) -> PathBuf {
    root.join("user-state/user.sqlite")
}

fn committed_iterations(user: &UserState) -> u64 {
    user.setting("harness.iterations")
        .unwrap()
        .map_or(0, |value| value.parse().unwrap())
}

/// Child entry point. A no-op in ordinary test runs; when re-executed by the
/// parent with `ROOT_ENV`, it alternates activations until it is killed.
#[test]
fn kill_harness_child() {
    let Ok(root) = std::env::var(ROOT_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    let store = ActiveSnapshotStore::new(&root);
    let user = UserState::open(&user_db(&root)).unwrap();
    let mut iterations = committed_iterations(&user);
    let mut announced = false;
    loop {
        for id in ["new", "old"] {
            store.switch_to_prevalidated(id).unwrap();
            iterations += 1;
            user.set_setting("harness.iterations", &iterations.to_string())
                .unwrap();
            user.set_reading_position(&key(), revision_for(id), iterations)
                .unwrap();
        }
        // Announce once via a marker file outside the snapshot root; libtest
        // child stdout is not reliably flushed before a SIGKILL.
        if !announced {
            fs::write(std::env::var(READY_ENV).unwrap(), b"ready").unwrap();
            announced = true;
        }
    }
}

fn private_selector_files(root: &Path) -> usize {
    fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".current.") || name.starts_with(".previous.")
        })
        .count()
}

fn assert_recoverable(root: &Path, inventories: &BTreeMap<&str, ShardInventory>) -> u64 {
    let active = ActiveSnapshotStore::new(root)
        .current()
        .expect("selector must stay readable after SIGKILL")
        .expect("an initialized store must keep a selector");
    let inventory = inventories
        .get(active.as_str())
        .unwrap_or_else(|| panic!("selector names unknown snapshot {active:?}"));
    let snapshot = root.join("snapshots").join(&active);
    let reader = VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &snapshot.join("catalog.sqlite"),
        &snapshot.join("search.sqlite"),
        &snapshot,
        inventory,
    )
    .expect("the selected snapshot must pass verified opening");
    let page = reader.reader().open_page(&key()).unwrap().unwrap();
    assert_eq!(page.revision.revision_id, revision_for(&active));
    // Both snapshots stay on disk: the inactive one remains a rollback target.
    for id in inventories.keys() {
        assert!(root
            .join("snapshots")
            .join(id)
            .join("manifest.json")
            .exists());
    }
    let user = UserState::open(&user_db(root)).expect("user state must reopen");
    assert_eq!(
        user.bookmark(&key()).unwrap(),
        Some((2001, "harness bookmark".into()))
    );
    committed_iterations(&user)
}

#[test]
fn sigkill_during_activation_never_corrupts_selector_content_or_user_state() {
    if std::env::var_os(ROOT_ENV).is_some() {
        return;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-kill-harness-{}-{stamp}", std::process::id()));
    let inventories = BTreeMap::from([
        ("old", stage_snapshot(&root, "old")),
        ("new", stage_snapshot(&root, "new")),
    ]);
    fs::create_dir_all(root.join("user-state")).unwrap();
    UserState::open(&user_db(&root))
        .unwrap()
        .set_bookmark(&key(), 2001, "harness bookmark")
        .unwrap();
    ActiveSnapshotStore::new(&root)
        .switch_to_prevalidated("old")
        .unwrap();

    let exe = std::env::current_exe().unwrap();
    let mut last_committed = 0;
    for kill in 0..KILLS {
        let ready = root.with_extension(format!("ready-{kill}"));
        let mut child = Command::new(&exe)
            .args([
                "--exact",
                "kill_harness_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ROOT_ENV, &root)
            .env(READY_ENV, &ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while !ready.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("child exited before activating (kill {kill}): {status}");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child never completed a first activation (kill {kill})"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        fs::remove_file(&ready).unwrap();
        // Stagger kill points across the sync/rename/rollback sequence.
        std::thread::sleep(Duration::from_micros(700 * (kill % 8) + 137 * kill));
        child.kill().unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success(), "child must die from SIGKILL");

        let committed = assert_recoverable(&root, &inventories);
        assert!(
            committed > last_committed,
            "kill {kill}: child made no committed progress or lost writes \
             ({committed} <= {last_committed})"
        );
        last_committed = committed;
    }

    // Leftover private files are never adopted and never block activation.
    let leftovers = private_selector_files(&root);
    let store = ActiveSnapshotStore::new(&root);
    for id in ["new", "old"] {
        store.switch_to_prevalidated(id).unwrap();
        assert_eq!(store.current().unwrap().as_deref(), Some(id));
    }
    assert!(private_selector_files(&root) <= leftovers);
    eprintln!("kill harness: {KILLS} kills, {last_committed} committed activations, {leftovers} orphaned private files");
    fs::remove_dir_all(root).unwrap();
}
