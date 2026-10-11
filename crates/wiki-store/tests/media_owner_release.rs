use wiki_store::media_ownership::{MediaNotice, MediaRegistry, OwnerKind};

#[test]
fn shared_ownership_survives_atomic_release() {
    let mut registry = MediaRegistry::in_memory().unwrap();
    let notice = MediaNotice {
        mime: "image/png".into(),
        source_url: "https://commons.wikimedia.org/wiki/File:Figure.png".into(),
        creator: "Contributor".into(),
        license: "CC BY-SA 4.0".into(),
    };
    let digest = registry
        .register_verified_bytes(b"figure", &notice)
        .unwrap();
    registry
        .add_owner(&digest, OwnerKind::Pack, "physics")
        .unwrap();
    registry
        .add_owner(&digest, OwnerKind::Pack, "math")
        .unwrap();
    assert_eq!(
        registry
            .remove_all_for_owner(OwnerKind::Pack, "physics")
            .unwrap(),
        vec![digest.clone()]
    );
    assert!(registry.unowned_digests().unwrap().is_empty());
    assert_eq!(
        registry.owned_media(OwnerKind::Pack, "math").unwrap().len(),
        1
    );
}
