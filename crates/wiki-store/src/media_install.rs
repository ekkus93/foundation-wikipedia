//! Install media bytes before recording durable ownership.
//!
//! This is a building block, not an offline-ready pack transaction: callers
//! must verify the authenticated manifest and every required visual before
//! activating a snapshot. An interrupted installation may leave unowned
//! content-addressed bytes; it must never make missing bytes look installed.

use crate::media_objects::{MediaObjectError, MediaObjectStore};
use crate::media_ownership::{MediaNotice, MediaRegistry, MediaRegistryError, OwnerKind};

#[derive(Debug)]
pub enum MediaInstallError {
    Store(MediaObjectError),
    Registry(MediaRegistryError),
    IdentityMismatch,
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
        assert_eq!(registry.unowned_digests().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
