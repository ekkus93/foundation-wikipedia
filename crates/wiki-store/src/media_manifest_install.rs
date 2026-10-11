//! Bind a caller-authenticated required-media manifest to an all-or-nothing
//! ownership batch. Authentication and visual discovery are caller obligations:
//! this module cannot declare an entire pack offline-ready.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::media_install::{install_owned_media_batch, MediaInstallError};
use crate::media_inventory::RequiredMedia;
use crate::media_objects::MediaObjectStore;
use crate::media_ownership::{MediaNotice, MediaRegistry, OwnerKind};

#[derive(Debug)]
pub enum ManifestMediaInstallError {
    InvalidManifest,
    InventoryMismatch,
    Install(MediaInstallError),
}

/// Require an exact digest-and-size match *before* storing any bytes or
/// creating any owner records. Repeated identical assets are permitted when
/// they carry distinct attribution notices, but required digests are unique.
///
/// The caller must independently authenticate `required` and prove that it
/// contains every visual referenced by the exact article revisions.
pub fn install_manifest_media_batch(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    assets: &[(&[u8], &MediaNotice)],
    required: &[RequiredMedia],
    kind: OwnerKind,
    owner_id: &str,
) -> Result<Vec<String>, ManifestMediaInstallError> {
    if required.is_empty()
        || required.len() > 100_000
        || assets.is_empty()
        || assets.len() > 100_000
    {
        return Err(ManifestMediaInstallError::InvalidManifest);
    }

    let mut expected = BTreeMap::new();
    let mut total = 0u64;
    for entry in required {
        if entry.digest.len() != 64
            || !entry
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || entry.bytes == 0
            || entry.bytes > 64 * 1024 * 1024
            || expected.insert(entry.digest.clone(), entry.bytes).is_some()
        {
            return Err(ManifestMediaInstallError::InvalidManifest);
        }
        total = total
            .checked_add(entry.bytes)
            .filter(|n| *n <= (1u64 << 40))
            .ok_or(ManifestMediaInstallError::InvalidManifest)?;
    }

    let mut actual = BTreeMap::new();
    for &(bytes, notice) in assets {
        if bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 || !notice.validate() {
            return Err(ManifestMediaInstallError::InvalidManifest);
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        actual.insert(digest, bytes.len() as u64);
    }
    if actual != expected {
        return Err(ManifestMediaInstallError::InventoryMismatch);
    }

    // This routine stages every verified content-addressed object before the
    // registry's single SQLite ownership transaction. Failed staging cannot
    // produce a partially installed owner.
    install_owned_media_batch(store, registry, assets, kind, owner_id)
        .map_err(ManifestMediaInstallError::Install)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_install::verify_required_owner_media;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn notice(creator: &str) -> MediaNotice {
        MediaNotice {
            mime: "image/svg+xml".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Figure.svg".into(),
            creator: creator.into(),
            license: "CC BY-SA 4.0".into(),
        }
    }

    fn entry(bytes: &[u8]) -> RequiredMedia {
        RequiredMedia {
            digest: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        }
    }

    #[test]
    fn exact_inventory_persists_shared_media_and_distinct_notices() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-manifest-install-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let db = root.join("registry.sqlite");
        let a = notice("Alice");
        let b = notice("Bob");
        let required = vec![entry(b"diagram"), entry(b"plot")];
        let assets: &[(&[u8], &MediaNotice)] = &[(b"diagram", &a), (b"plot", &a), (b"diagram", &b)];
        let mut registry = MediaRegistry::open(&db).unwrap();
        let installed = install_manifest_media_batch(
            &store,
            &mut registry,
            assets,
            &required,
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        assert_eq!(installed[0], installed[2]);
        assert_eq!(registry.notice_count(&installed[0]).unwrap(), 2);
        registry
            .add_owner(&installed[0], OwnerKind::UserPin, "reader")
            .unwrap();
        drop(registry);

        let registry = MediaRegistry::open(&db).unwrap();
        assert_eq!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required)
                .unwrap(),
            11,
        );
        registry
            .remove_owner(&installed[0], OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .remove_owner(&installed[1], OwnerKind::Pack, "physics")
            .unwrap();
        assert_eq!(
            registry.unowned_digests().unwrap(),
            vec![installed[1].clone()]
        );
        assert_eq!(store.read_verified(&installed[0]).unwrap(), b"diagram");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_extra_duplicate_or_wrong_size_manifest_does_not_write() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-manifest-reject-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        let n = notice("Alice");
        let assets: &[(&[u8], &MediaNotice)] = &[(b"diagram", &n), (b"plot", &n)];
        let one = entry(b"diagram");
        let two = entry(b"plot");
        let mut wrong = one.clone();
        wrong.bytes += 1;
        let mut uppercase = one.clone();
        uppercase.digest = uppercase.digest.to_uppercase();
        for (manifest, invalid) in [
            (vec![one.clone()], false),
            (vec![one.clone(), two.clone(), entry(b"missing")], false),
            (vec![wrong, two.clone()], false),
            (vec![one.clone(), one.clone()], true),
            (vec![uppercase, two.clone()], true),
        ] {
            let result = install_manifest_media_batch(
                &store,
                &mut registry,
                assets,
                &manifest,
                OwnerKind::Pack,
                "physics",
            );
            assert!(matches!(
                result,
                Err(ManifestMediaInstallError::InvalidManifest)
                    | Err(ManifestMediaInstallError::InventoryMismatch)
            ));
            assert_eq!(
                matches!(result, Err(ManifestMediaInstallError::InvalidManifest)),
                invalid
            );
            assert!(registry
                .owned_media(OwnerKind::Pack, "physics")
                .unwrap()
                .is_empty());
            assert!(registry.unowned_digests().unwrap().is_empty());
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
