//! Install media bytes before recording durable ownership.
//!
//! This is a building block, not an offline-ready pack transaction: callers
//! must verify the authenticated manifest and every required visual before
//! activating a snapshot. An interrupted installation may leave unowned
//! content-addressed bytes; it must never make missing bytes look installed.

use crate::media_inventory::{verify_declared_media, MediaInventoryError, RequiredMedia};
use crate::media_objects::{MediaObjectError, MediaObjectStore};
use crate::media_ownership::{
    valid_owner_id, MediaNotice, MediaRegistry, MediaRegistryError, OwnerKind,
};

#[derive(Debug)]
pub enum MediaInstallError {
    Store(MediaObjectError),
    Registry(MediaRegistryError),
    IdentityMismatch,
    Inventory(MediaInventoryError),
}

/// Persist and rehash the object first, then register attribution and owner.
/// A failure before the final ownership write must not activate a pack.
pub fn install_owned_media(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    content: &[u8],
    notice: &MediaNotice,
    kind: OwnerKind,
    owner_id: &str,
) -> Result<String, MediaInstallError> {
    if !valid_owner_id(owner_id) || !notice.validate() {
        return Err(MediaInstallError::Registry(
            MediaRegistryError::InvalidInput,
        ));
    }
    let stored_digest = store
        .store_bytes(content)
        .map_err(MediaInstallError::Store)?;
    let registered_digest = registry
        .register_verified_bytes(content, notice)
        .map_err(MediaInstallError::Registry)?;
    if stored_digest != registered_digest {
        return Err(MediaInstallError::IdentityMismatch);
    }
    registry
        .add_owner(&stored_digest, kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    Ok(stored_digest)
}

/// Stage every object before atomically registering attribution and ownership
/// for the complete batch. A failed stage may leave unowned deduplicated bytes,
/// but cannot leave a partially registered pack. This is not pack activation:
/// the caller must authenticate the manifest and verify required visuals.
pub fn install_owned_media_batch(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    assets: &[(&[u8], &MediaNotice)],
    kind: OwnerKind,
    owner_id: &str,
) -> Result<Vec<String>, MediaInstallError> {
    if !valid_owner_id(owner_id)
        || assets.is_empty()
        || assets.len() > 100_000
        || assets.iter().any(|(bytes, notice)| {
            bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 || !notice.validate()
        })
    {
        return Err(MediaInstallError::Registry(
            MediaRegistryError::InvalidInput,
        ));
    }
    let mut staged = Vec::with_capacity(assets.len());
    for &(content, _) in assets {
        staged.push(
            store
                .store_bytes(content)
                .map_err(MediaInstallError::Store)?,
        );
    }
    let registered = registry
        .register_verified_owned_batch(assets, kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    if registered != staged {
        return Err(MediaInstallError::IdentityMismatch);
    }
    Ok(staged)
}

/// Rehash every registered object for one owner. An authenticated pack
/// manifest must separately prove that this owner has *all* required media.
pub fn verify_registered_owner_bytes(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
) -> Result<u64, MediaInstallError> {
    let claims = registry
        .owned_media(kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    let entries: Vec<RequiredMedia> = claims
        .into_iter()
        .map(|(digest, bytes)| RequiredMedia { digest, bytes })
        .collect();
    verify_declared_media(store, &entries).map_err(MediaInstallError::Inventory)
}

/// Compare an authenticated manifest's required-media inventory with exactly
/// the owner's registered objects, then rehash every on-disk object. Neither
/// this method nor registry membership authenticates the manifest itself or
/// proves the caller discovered every visual used by the article renderer.
pub fn verify_required_owner_media(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    required: &[RequiredMedia],
) -> Result<u64, MediaInstallError> {
    let registered = registry
        .owned_media(kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    if registered.len() != required.len() {
        return Err(MediaInstallError::IdentityMismatch);
    }
    let mut expected: Vec<_> = required
        .iter()
        .map(|entry| (entry.digest.as_str(), entry.bytes))
        .collect();
    expected.sort_unstable();
    if !registered
        .iter()
        .map(|(digest, bytes)| (digest.as_str(), *bytes))
        .eq(expected.into_iter())
    {
        return Err(MediaInstallError::IdentityMismatch);
    }
    verify_declared_media(store, required).map_err(MediaInstallError::Inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_inventory::{verify_declared_media, MediaInventoryError, RequiredMedia};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn notice(creator: &str) -> MediaNotice {
        MediaNotice {
            mime: "image/svg+xml".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Diagram.svg".into(),
            creator: creator.into(),
            license: "CC BY-SA 4.0".into(),
        }
    }

    #[test]
    fn batch_ownership_is_atomic_and_persists_across_reopen() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("wiki-batch-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let db = root.join("owners.sqlite");
        let a = notice("Creator A");
        let b = notice("Creator B");
        let bad = MediaNotice {
            license: " ".into(),
            ..notice("Bad")
        };
        let mut registry = MediaRegistry::open(&db).unwrap();
        let first: &[(&[u8], &MediaNotice)] = &[(b"diagram", &a), (b"figure", &bad)];
        assert!(matches!(
            install_owned_media_batch(&store, &mut registry, first, OwnerKind::Pack, "physics"),
            Err(MediaInstallError::Registry(
                MediaRegistryError::InvalidInput
            ))
        ));
        assert!(registry
            .owned_media(OwnerKind::Pack, "physics")
            .unwrap()
            .is_empty());
        assert!(registry.unowned_digests().unwrap().is_empty());

        let assets: &[(&[u8], &MediaNotice)] =
            &[(b"diagram", &a), (b"figure", &b), (b"diagram", &b)];
        let digests =
            install_owned_media_batch(&store, &mut registry, assets, OwnerKind::Pack, "physics")
                .unwrap();
        assert_eq!(digests[0], digests[2]);
        assert_eq!(registry.notice_count(&digests[0]).unwrap(), 2);
        assert_eq!(
            registry
                .owned_media(OwnerKind::Pack, "physics")
                .unwrap()
                .len(),
            2
        );
        registry
            .add_owner(&digests[0], OwnerKind::UserPin, "reader")
            .unwrap();
        drop(registry);

        let registry = MediaRegistry::open(&db).unwrap();
        let required = vec![
            RequiredMedia {
                digest: digests[0].clone(),
                bytes: 7,
            },
            RequiredMedia {
                digest: digests[1].clone(),
                bytes: 6,
            },
        ];
        assert_eq!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required)
                .unwrap(),
            13
        );
        registry
            .remove_owner(&digests[0], OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .remove_owner(&digests[1], OwnerKind::Pack, "physics")
            .unwrap();
        assert_eq!(
            registry.unowned_digests().unwrap(),
            vec![digests[1].clone()]
        );
        assert_eq!(store.read_verified(&digests[0]).unwrap(), b"diagram");
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_media_is_shared_and_ownership_survives_pack_removal() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-install-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        let digest = install_owned_media(
            &store,
            &mut registry,
            b"diagram",
            &notice("Creator A"),
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        assert_eq!(
            install_owned_media(
                &store,
                &mut registry,
                b"diagram",
                &notice("Creator B"),
                OwnerKind::Pack,
                "math",
            )
            .unwrap(),
            digest
        );
        assert_eq!(registry.notice_count(&digest).unwrap(), 2);
        registry
            .remove_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        assert!(registry.unowned_digests().unwrap().is_empty());
        assert_eq!(
            verify_registered_owner_bytes(&store, &registry, OwnerKind::Pack, "math").unwrap(),
            7
        );
        assert_eq!(
            verify_declared_media(
                &store,
                &[RequiredMedia {
                    digest: digest.clone(),
                    bytes: 7,
                }]
            )
            .unwrap(),
            7
        );
        registry
            .remove_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert_eq!(registry.unowned_digests().unwrap(), vec![digest.clone()]);
        fs::write(root.join(&digest[..2]).join(&digest), b"changed").unwrap();
        assert!(matches!(
            verify_declared_media(&store, &[RequiredMedia { digest, bytes: 7 }]),
            Err(MediaInventoryError::Unreadable(_))
        ));
        assert!(matches!(
            verify_registered_owner_bytes(&store, &registry, OwnerKind::Pack, "physics"),
            Ok(0)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn required_media_must_match_owner_claims_before_acceptance() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-required-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        let shared = install_owned_media(
            &store,
            &mut registry,
            b"diagram",
            &notice("Creator"),
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        let additional = install_owned_media(
            &store,
            &mut registry,
            b"figure",
            &notice("Creator"),
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        let required = vec![
            RequiredMedia {
                digest: additional.clone(),
                bytes: 6,
            },
            RequiredMedia {
                digest: shared.clone(),
                bytes: 7,
            },
        ];
        assert_eq!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required)
                .unwrap(),
            13
        );
        assert!(matches!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "missing", &required),
            Err(MediaInstallError::IdentityMismatch)
        ));
        assert!(matches!(
            verify_required_owner_media(
                &store,
                &registry,
                OwnerKind::Pack,
                "physics",
                &required[..1]
            ),
            Err(MediaInstallError::IdentityMismatch)
        ));
        let mut wrong_size = required.clone();
        wrong_size[0].bytes = 9;
        assert!(matches!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &wrong_size),
            Err(MediaInstallError::IdentityMismatch)
        ));
        fs::write(root.join(&shared[..2]).join(&shared), b"tampered").unwrap();
        assert!(matches!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required),
            Err(MediaInstallError::Inventory(
                MediaInventoryError::Unreadable(_)
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_owner_never_becomes_an_installed_owner() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-install-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        assert!(matches!(
            install_owned_media(
                &store,
                &mut registry,
                b"diagram",
                &notice("Creator"),
                OwnerKind::Pack,
                "../invalid",
            ),
            Err(MediaInstallError::Registry(
                MediaRegistryError::InvalidInput
            ))
        ));
        assert!(registry.unowned_digests().unwrap().is_empty());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
