//! MOB-001 host tests of the Kotlin-callable offline library API.

use std::fs;
use std::path::{Path, PathBuf};

use wiki_ffi::android::{LibraryError, OfflineLibrary};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::PackMetadata;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::pack_install::snapshot_pack_metadata;
use wiki_store::record_codec::append_record_frame;

struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_dir(label: &str) -> TestDir {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "wiki-ffi-android-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    TestDir(dir)
}

fn article(page_id: u64, title: &str, text: &str) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 7000 + page_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
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
            content: BlockContent::Paragraph(text.into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: format!("<article><h1>{title}</h1><p>{text}</p></article>"),
        is_disambiguation: false,
    }
}

/// Build a two-article pack file and return its path.
fn build_pack(dir: &Path, pack_id: &str) -> PathBuf {
    let tree = dir.join(format!("{pack_id}-tree"));
    fs::create_dir_all(&tree).unwrap();
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for article in [
        article(1, "Moon", "The Moon orbits the Earth."),
        article(2, "Tide", "Tides follow the Moon's gravity."),
    ] {
        let offset = shard.len() as u64;
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article))).unwrap();
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    for (offset, bytes) in frames {
        catalog
            .insert_verified(&tree, "articles.shard", offset, bytes)
            .unwrap();
    }
    drop(catalog);
    let metadata = snapshot_pack_metadata(
        &tree,
        PackMetadata::synthetic_fixture().source,
        &"0".repeat(64),
        "CC-BY-SA-4.0",
    )
    .unwrap();
    let manifest =
        custom_manifest_from_tree(&tree, pack_id, "enwiki", "20261101", "moon", metadata).unwrap();
    let path = dir.join(format!("{pack_id}.wpack"));
    let mut file = fs::File::create(&path).unwrap();
    write_custom_tree(&tree, &manifest, &mut file).unwrap();
    path
}

fn library(dir: &Path) -> std::sync::Arc<OfflineLibrary> {
    OfflineLibrary::new(dir.join("library").to_string_lossy().into_owned()).unwrap()
}

#[test]
fn install_activate_search_and_open_through_the_exported_api() {
    let dir = test_dir("lifecycle");
    let pack = build_pack(&dir.0, "moon-pack");
    let pack_path = pack.to_string_lossy().into_owned();
    let library = library(&dir.0);

    assert_eq!(library.active_snapshot().unwrap(), None);
    assert_eq!(
        library.search("moon".into(), 10),
        Err(LibraryError::NotInstalled)
    );
    assert_eq!(
        library.install_pack(pack_path.clone(), false),
        Err(LibraryError::ConfirmationRequired)
    );
    let installed = library.install_pack(pack_path.clone(), true).unwrap();
    assert_eq!(installed.snapshot_id, "moon-pack");
    assert_eq!(installed.project, "enwiki");
    assert_eq!(installed.trust_label, "Custom — integrity verified");
    assert_eq!(installed.articles, 2);
    assert_eq!(
        library.install_pack(pack_path, true),
        Err(LibraryError::AlreadyInstalled)
    );

    library.activate("moon-pack".into()).unwrap();
    assert_eq!(
        library.active_snapshot().unwrap().as_deref(),
        Some("moon-pack")
    );
    let hits = library.search("gravity".into(), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].page_id, hits[0].title.as_str()), (2, "Tide"));
    let page = library.open_page("enwiki".into(), 1).unwrap().unwrap();
    assert_eq!(page.title, "Moon");
    assert_eq!(page.revision_id, 7001);
    assert!(page.html.contains("orbits the Earth"));
    assert_eq!(library.open_page("enwiki".into(), 99).unwrap(), None);

    // A new instance (app restart) reopens the active snapshot from disk.
    drop(library);
    let restarted = self::library(&dir.0);
    assert_eq!(
        restarted.active_snapshot().unwrap().as_deref(),
        Some("moon-pack")
    );
    assert_eq!(restarted.search("moon".into(), 10).unwrap().len(), 2);
}

#[test]
fn invalid_input_and_corruption_fail_closed() {
    let dir = test_dir("invalid");
    assert!(matches!(
        OfflineLibrary::new("relative/dir".into()),
        Err(LibraryError::InvalidInput)
    ));
    let library = library(&dir.0);
    assert_eq!(
        library.install_pack("relative.wpack".into(), true),
        Err(LibraryError::InvalidInput)
    );
    assert_eq!(
        library.install_pack(dir.0.join("missing.wpack").to_string_lossy().into(), true),
        Err(LibraryError::InvalidInput)
    );
    let garbage = dir.0.join("garbage.wpack");
    fs::write(&garbage, b"not a pack").unwrap();
    assert_eq!(
        library.install_pack(garbage.to_string_lossy().into(), true),
        Err(LibraryError::VerificationFailed)
    );
    assert_eq!(
        library.activate("absent".into()),
        Err(LibraryError::NotInstalled)
    );
    assert_eq!(
        library.open_page("Not A Project".into(), 1),
        Err(LibraryError::InvalidInput)
    );

    let pack = build_pack(&dir.0, "moon-pack");
    library
        .install_pack(pack.to_string_lossy().into(), true)
        .unwrap();
    library.activate("moon-pack".into()).unwrap();
    for (query, limit) in [("", 5), ("   ", 5), ("moon", 0)] {
        assert_eq!(
            library.search(query.into(), limit),
            Err(LibraryError::InvalidInput)
        );
    }
    assert_eq!(
        library.search("x".repeat(1025), 5),
        Err(LibraryError::InvalidInput)
    );

    // Corrupt the installed shard: a restarted library must refuse it.
    let shard = dir.0.join("library/snapshots/moon-pack/articles.shard");
    let mut bytes = fs::read(&shard).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&shard, bytes).unwrap();
    drop(library);
    let restarted = self::library(&dir.0);
    assert_eq!(
        restarted.search("moon".into(), 5),
        Err(LibraryError::VerificationFailed)
    );
}

fn android_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/wpack/android-sample-v2.wpack")
}

/// The Android instrumented test installs this committed pack. It must stay
/// exactly what the builder above produces; regenerate after an intentional
/// change with `WPACK_WRITE_FIXTURE=1 cargo test -p wiki-ffi --test
/// android_library committed_android_fixture` and update its provenance hash.
#[test]
fn committed_android_fixture_matches_the_builder() {
    let first = test_dir("fixture-a");
    let second = test_dir("fixture-b");
    let bytes = fs::read(build_pack(&first.0, "android-sample")).unwrap();
    assert_eq!(
        bytes,
        fs::read(build_pack(&second.0, "android-sample")).unwrap(),
        "the pack builder must be deterministic"
    );
    if std::env::var_os("WPACK_WRITE_FIXTURE").is_some() {
        fs::write(android_fixture_path(), &bytes).unwrap();
    }
    assert_eq!(fs::read(android_fixture_path()).unwrap(), bytes);
}
