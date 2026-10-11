use wiki_store::media_ownership::{MediaNotice, MediaRegistry, MediaRegistryError, OwnerKind};

fn notice() -> MediaNotice {
    MediaNotice {
        mime: "image/png".into(),
        source_url: "https://commons.wikimedia.org/wiki/File:Figure.png".into(),
        creator: "Wikimedia contributor".into(),
        license: "CC BY-SA 4.0".into(),
    }
}

#[test]
fn release_entire_pack_preserves_shared_and_pinned_media() {
    let mut registry = MediaRegistry::in_memory().unwrap();
    let a = registry.register_verified_bytes(b"shared", &notice()).unwrap();
    let b = registry.register_verified_bytes(b"exclusive", &notice()).unwrap();
    for digest in [&a, &b] {
        registry.add_owner(digest, OwnerKind::Pack, "physics").unwrap();
    }
    registry.add_owner(&a, OwnerKind::Pack, "math").unwrap();
    registry.add_owner(&b, OwnerKind::UserPin, "reader").unwrap();

    let mut expected = vec![a.clone(), b.clone()];
    expected.sort();
    assert_eq!(registry.remove_all_for_owner(OwnerKind::Pack, "physics").unwrap(), expected);
    assert!(registry.owned_media(OwnerKind::Pack, "physics").unwrap().is_empty());
    assert!(registry.unowned_digests().unwrap().is_empty());
    assert!(registry.remove_all_for_owner(OwnerKind::Pack, "physics").unwrap().is_empty());
    assert_eq!(registry.owned_media(OwnerKind::Pack, "math").unwrap().len(), 1);

    registry.remove_all_for_owner(OwnerKind::UserPin, "reader").unwrap();
    assert_eq!(registry.unowned_digests().unwrap(), vec![b]);
    registry.remove_all_for_owner(OwnerKind::Pack, "math").unwrap();
    assert_eq!(registry.unowned_digests().unwrap().len(), 2);
}

#[test]
fn invalid_owner_cannot_release_anything() {
    let mut registry = MediaRegistry::in_memory().unwrap();
    let digest = registry.register_verified_bytes(b"diagram", &notice()).unwrap();
    registry.add_owner(&digest, OwnerKind::Pack, "physics").unwrap();
    assert!(matches!(
        registry.remove_all_for_owner(OwnerKind::Pack, "../physics"),
        Err(MediaRegistryError::InvalidInput)
    ));
    assert_eq!(registry.owned_media(OwnerKind::Pack, "physics").unwrap().len(), 1);
}
