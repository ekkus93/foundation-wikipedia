use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_store::media_ownership::{MediaNotice, MediaRegistry, OwnerKind};

#[test]
fn durable_media_ownership_keeps_shared_bytes_until_last_owner_releases() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-media-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let database = root.join("owners.sqlite");
    let notice = MediaNotice {
        mime: "image/svg+xml".into(),
        source_url: "https://commons.wikimedia.org/wiki/File:Example.svg".into(),
        creator: "Contributor".into(),
        license: "CC BY-SA 4.0".into(),
    };
    let digest = {
        let mut registry = MediaRegistry::open(&database).unwrap();
        let digest = registry
            .register_verified_bytes(b"sample SVG bytes", &notice)
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Cache, "session")
            .unwrap();
        digest
    };
    let registry = MediaRegistry::open(&database).unwrap();
    assert_eq!(registry.notice_count(&digest).unwrap(), 1);
    registry
        .remove_owner(&digest, OwnerKind::Pack, "physics")
        .unwrap();
    registry
        .remove_owner(&digest, OwnerKind::Pack, "math")
        .unwrap();
    assert!(registry.unowned_digests().unwrap().is_empty());
    registry
        .remove_owner(&digest, OwnerKind::Cache, "session")
        .unwrap();
    assert_eq!(registry.unowned_digests().unwrap(), vec![digest]);
    drop(registry);
    fs::remove_dir_all(root).unwrap();
}
