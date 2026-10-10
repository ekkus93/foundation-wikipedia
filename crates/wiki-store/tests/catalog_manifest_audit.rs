use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wiki_model::{ArticleKey, PageRecord, Redirect};
use wiki_store::catalog::{CatalogError, SnapshotCatalog};
use wiki_store::record_codec::append_record_frame;

#[test]
fn full_manifest_audit_requires_every_indexed_shard_and_exact_hash() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-manifest-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut catalog = SnapshotCatalog::in_memory().unwrap();
    let mut expected = BTreeMap::new();
    for (page_id, name) in [(10, "first.shard"), (11, "second.shard")] {
        let record = PageRecord::Redirect(Redirect {
            from: ArticleKey {
                project: "enwiki".into(),
                page_id,
            },
            title: format!("Redirect {page_id}"),
            to: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
        });
        let mut frame = Vec::new();
        append_record_frame(&mut frame, &record).unwrap();
        fs::write(root.join(name), &frame).unwrap();
        catalog
            .insert_verified(&root, name, 0, frame.len() as u64)
            .unwrap();
        expected.insert(
            name.to_owned(),
            (frame.len() as u64, format!("{:x}", Sha256::digest(&frame))),
        );
    }
    assert_eq!(
        catalog
            .verify_manifest_shard_hashes(&root, &expected)
            .unwrap(),
        2
    );

    let mut incomplete = expected.clone();
    incomplete.remove("second.shard");
    assert!(matches!(
        catalog.verify_manifest_shard_hashes(&root, &incomplete),
        Err(CatalogError::InvalidCatalogEntry)
    ));
    let mut extra = expected.clone();
    extra.insert("unexpected.shard".into(), (0, "0".repeat(64)));
    assert!(matches!(
        catalog.verify_manifest_shard_hashes(&root, &extra),
        Err(CatalogError::InvalidCatalogEntry)
    ));
    let mut wrong_digest = expected.clone();
    wrong_digest.get_mut("second.shard").unwrap().1 = "0".repeat(64);
    assert!(matches!(
        catalog.verify_manifest_shard_hashes(&root, &wrong_digest),
        Err(CatalogError::DigestMismatch)
    ));
    fs::remove_dir_all(root).unwrap();
}
