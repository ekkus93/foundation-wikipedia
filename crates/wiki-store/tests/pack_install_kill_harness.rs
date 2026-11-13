//! STORE-003 process-interruption harness for the custom pack installer (Unix).
//!
//! The test binary re-executes itself as a child that installs prebuilt
//! custom packs one after another; the parent SIGKILLs it at staggered
//! points. After every kill, each directory under `snapshots/` must be a
//! complete snapshot that passes verified opening, residue may exist only in
//! the private `staging/` directory, and `reclaim_abandoned_staging` must
//! clear that residue without touching published snapshots.
//!
//! This exercises abrupt process death only, NOT power loss.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_store::activation::ActiveSnapshotStore;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::offline_reader::VerifiedOfflineCatalogReader;
use wiki_store::pack_install::{
    install_custom_pack, reclaim_abandoned_staging, snapshot_pack_metadata, PackInstallError,
    ShardInventory,
};
use wiki_store::record_codec::append_record_frame;

const ROOT_ENV: &str = "WIKI_STORE_INSTALL_HARNESS_ROOT";
const PACKS_ENV: &str = "WIKI_STORE_INSTALL_HARNESS_PACKS";
const READY_ENV: &str = "WIKI_STORE_INSTALL_HARNESS_READY";
const KILLS: u64 = 16;
const PACKS: usize = 400;

fn key() -> ArticleKey {
    ArticleKey {
        project: "enwiki".into(),
        page_id: 9,
    }
}

/// One snapshot-layout tree shared by every pack (packs differ by ID only),
/// built without a search index so each install also rebuilds one.
fn build_tree(tree: &Path) -> ShardInventory {
    fs::create_dir_all(tree).unwrap();
    let article = Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: key(),
        revision: Revision {
            revision_id: 4001,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "d".repeat(64),
        },
        title: "Venus".into(),
        display_title: "Venus".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph("the morning star".into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>Venus</article>".into(),
        is_disambiguation: false,
    };
    let mut shard = Vec::new();
    append_record_frame(&mut shard, &PageRecord::Article(Box::new(article))).unwrap();
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    catalog
        .insert_verified(tree, "articles.shard", 0, shard.len() as u64)
        .unwrap();
    use sha2::{Digest, Sha256};
    BTreeMap::from([(
        "articles.shard".to_owned(),
        (shard.len() as u64, format!("{:x}", Sha256::digest(&shard))),
    )])
}

fn pack_id(n: usize) -> String {
    format!("venus-{n:04}")
}

/// Child entry point: a no-op in ordinary test runs.
#[test]
fn install_harness_child() {
    let Ok(root) = std::env::var(ROOT_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    let packs = PathBuf::from(std::env::var(PACKS_ENV).unwrap());
    let mut announced = false;
    for n in 0..PACKS {
        let bytes = fs::read(packs.join(format!("{}.wpack", pack_id(n)))).unwrap();
        match install_custom_pack(&root, &mut Cursor::new(bytes), 1 << 30, |_| true) {
            Ok(_) => {}
            // Installed by an earlier, killed child: not progress.
            Err(PackInstallError::AlreadyInstalled) => continue,
            Err(error) => panic!("install {n} failed: {error:?}"),
        }
        // Announce only after this child published a new snapshot.
        if !announced {
            fs::write(std::env::var(READY_ENV).unwrap(), b"ready").unwrap();
            announced = true;
        }
    }
    // Exhausted: wait to be killed rather than exiting successfully.
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn audit_snapshots(root: &Path, inventory: &ShardInventory) -> usize {
    let mut count = 0;
    for entry in fs::read_dir(root.join("snapshots")).unwrap() {
        let snapshot = entry.unwrap().path();
        assert!(
            snapshot.join("manifest.json").is_file(),
            "{snapshot:?} published without its installer manifest"
        );
        let reader = VerifiedOfflineCatalogReader::open_with_manifest_shards(
            &snapshot.join("catalog.sqlite"),
            &snapshot.join("search.sqlite"),
            &snapshot,
            inventory,
        )
        .unwrap_or_else(|error| panic!("{snapshot:?} is not a complete snapshot: {error:?}"));
        let page = reader.reader().open_page(&key()).unwrap().unwrap();
        assert_eq!(page.revision.revision_id, 4001);
        count += 1;
    }
    count
}

#[test]
fn sigkill_during_install_never_publishes_a_partial_snapshot() {
    if std::env::var_os(ROOT_ENV).is_some() {
        return;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "wiki-install-harness-{}-{stamp}",
        std::process::id()
    ));
    let tree = base.join("tree");
    let packs = base.join("packs");
    let root = base.join("root");
    let inventory = build_tree(&tree);
    fs::create_dir_all(&packs).unwrap();
    fs::create_dir_all(&root).unwrap();
    let metadata = snapshot_pack_metadata(
        &tree,
        wiki_pack_format::manifest::PackMetadata::synthetic_fixture().source,
        &"0".repeat(64),
        "CC-BY-SA-4.0",
    )
    .unwrap();
    for n in 0..PACKS {
        let manifest = custom_manifest_from_tree(
            &tree,
            &pack_id(n),
            "enwiki",
            "20261101",
            "venus",
            metadata.clone(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        write_custom_tree(&tree, &manifest, &mut bytes).unwrap();
        fs::write(packs.join(format!("{}.wpack", pack_id(n))), bytes).unwrap();
    }

    let exe = std::env::current_exe().unwrap();
    let mut last_installed = 0;
    let mut reclaimed_total = 0;
    for kill in 0..KILLS {
        let ready = base.join(format!("ready-{kill}"));
        let mut child = Command::new(&exe)
            .args([
                "--exact",
                "install_harness_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ROOT_ENV, &root)
            .env(PACKS_ENV, &packs)
            .env(READY_ENV, &ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !ready.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("child exited before installing (kill {kill}): {status}");
            }
            assert!(
                Instant::now() < deadline,
                "child never installed (kill {kill})"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_micros(900 * (kill % 8) + 211 * kill));
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());

        let installed = audit_snapshots(&root, &inventory);
        assert!(
            installed > last_installed,
            "kill {kill}: no install progress ({installed} <= {last_installed})"
        );
        last_installed = installed;
        // The child is dead, so the parent now owns the root exclusively.
        let (removed, retained) = reclaim_abandoned_staging(&root).unwrap();
        assert_eq!(retained, 0);
        reclaimed_total += removed;
        assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
        assert_eq!(audit_snapshots(&root, &inventory), installed);
    }

    // Every surviving snapshot is still activatable.
    let store = ActiveSnapshotStore::new(&root);
    store.switch_to_prevalidated(&pack_id(0)).unwrap();
    assert_eq!(store.current().unwrap().as_deref(), Some("venus-0000"));
    eprintln!(
        "install harness: {KILLS} kills, {last_installed} installed snapshots, \
         {reclaimed_total} reclaimed staging entries"
    );
    fs::remove_dir_all(base).unwrap();
}
