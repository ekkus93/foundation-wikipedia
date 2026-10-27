//! Verify declared offline media bytes before pack activation.
//! The caller must independently authenticate the manifest and prove that
//! every required visual was declared. This check only verifies local bytes.

use crate::media_lease::MediaLeaseCoordinator;
use crate::media_objects::{MediaObjectError, MediaObjectStore};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredMedia {
    pub digest: String,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum MediaInventoryError {
    InvalidManifest,
    DuplicateDigest,
    Unreadable(MediaObjectError),
    WrongSize,
}

/// Rehash every declared object under one shared media lease. An exclusive
/// collector cannot unlink earlier inventory members during later checks.
pub fn verify_declared_media(
    store: &MediaObjectStore,
    entries: &[RequiredMedia],
) -> Result<u64, MediaInventoryError> {
    let total = validate_inventory(entries)?;
    if entries.is_empty() {
        return Ok(0);
    }
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(|error| MediaInventoryError::Unreadable(MediaObjectError::Lease(error)))?;
    verify_entries_unlocked(store, entries, total)
}

/// The caller must already hold the shared/exclusive lease. This avoids
/// nested SQLite locks during owner inventory and manifest verification.
pub(crate) fn verify_declared_media_unlocked(
    store: &MediaObjectStore,
    entries: &[RequiredMedia],
) -> Result<u64, MediaInventoryError> {
    let total = validate_inventory(entries)?;
    verify_entries_unlocked(store, entries, total)
}

fn validate_inventory(entries: &[RequiredMedia]) -> Result<u64, MediaInventoryError> {
    if entries.len() > 100_000 {
        return Err(MediaInventoryError::InvalidManifest);
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for entry in entries {
        if entry.digest.len() != 64
            || !entry
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || entry.bytes == 0
            // Match MediaObjectStore's per-object hard cap. Reject impossible
            // size claims before opening or hashing any manifest members.
            || entry.bytes > 64 * 1024 * 1024
        {
            return Err(MediaInventoryError::InvalidManifest);
        }
        if !seen.insert(&entry.digest) {
            return Err(MediaInventoryError::DuplicateDigest);
        }
        total = total
            .checked_add(entry.bytes)
            .filter(|n| *n <= (1u64 << 40))
            .ok_or(MediaInventoryError::InvalidManifest)?;
    }
    Ok(total)
}

fn verify_entries_unlocked(
    store: &MediaObjectStore,
    entries: &[RequiredMedia],
    total: u64,
) -> Result<u64, MediaInventoryError> {
    for entry in entries {
        let bytes = store
            .verify_object_size_unlocked(&entry.digest)
            .map_err(MediaInventoryError::Unreadable)?;
        if bytes != entry.bytes {
            return Err(MediaInventoryError::WrongSize);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn inventory_verification_waits_for_exclusive_lease() {
        use std::sync::mpsc;
        use std::time::Duration;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-inventory-lease-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let entries = [b"first visual".as_slice(), b"second visual".as_slice()]
            .iter()
            .map(|bytes| RequiredMedia {
                digest: store.store_bytes(bytes).unwrap(),
                bytes: bytes.len() as u64,
            })
            .collect::<Vec<_>>();
        let exclusive = MediaLeaseCoordinator::new(&root)
            .acquire_exclusive()
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker_root = root.clone();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            finished_tx
                .send(verify_declared_media(
                    &MediaObjectStore::new(worker_root),
                    &entries,
                ))
                .unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(finished_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        drop(exclusive);
        assert_eq!(
            finished_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap(),
            25
        );
        worker.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inventory_verifies_bytes_and_rejects_corruption() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-inventory-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"diagram").unwrap();
        let item = RequiredMedia {
            digest: digest.clone(),
            bytes: 7,
        };
        assert_eq!(verify_declared_media(&store, &[item.clone()]).unwrap(), 7);
        assert!(matches!(
            verify_declared_media(&store, &[item.clone(), item.clone()]),
            Err(MediaInventoryError::DuplicateDigest)
        ));
        assert!(matches!(
            verify_declared_media(
                &store,
                &[RequiredMedia {
                    bytes: 8,
                    ..item.clone()
                }]
            ),
            Err(MediaInventoryError::WrongSize)
        ));
        assert!(matches!(
            verify_declared_media(
                &store,
                &[RequiredMedia {
                    bytes: 64 * 1024 * 1024 + 1,
                    ..item.clone()
                }]
            ),
            Err(MediaInventoryError::InvalidManifest)
        ));
        fs::write(root.join(&digest[..2]).join(&digest), b"tampered").unwrap();
        assert!(matches!(
            verify_declared_media(&store, &[item]),
            Err(MediaInventoryError::Unreadable(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn large_media_inventory_streams_and_rejects_same_size_corruption() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-inventory-stream-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let bytes = vec![0x5a; 4 * 1024 * 1024 + 13];
        let digest = store.store_bytes(&bytes).unwrap();
        let entry = RequiredMedia {
            digest: digest.clone(),
            bytes: bytes.len() as u64,
        };
        assert_eq!(store.verify_object_size(&digest).unwrap(), entry.bytes);
        assert_eq!(
            verify_declared_media(&store, &[entry.clone()]).unwrap(),
            entry.bytes
        );
        let path = root.join(&digest[..2]).join(&digest);
        let mut changed = bytes;
        let last = changed.len() - 1;
        changed[last] ^= 1;
        fs::write(&path, changed).unwrap();
        assert!(matches!(
            verify_declared_media(&store, &[entry]),
            Err(MediaInventoryError::Unreadable(
                MediaObjectError::DigestMismatch
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn streaming_media_audit_rejects_external_hardlinks() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-inventory-link-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"offline figure").unwrap();
        let alias = root.join("external-alias");
        fs::hard_link(root.join(&digest[..2]).join(&digest), &alias).unwrap();
        let entry = RequiredMedia { digest, bytes: 14 };
        assert!(matches!(
            verify_declared_media(&store, &[entry]),
            Err(MediaInventoryError::Unreadable(
                MediaObjectError::UnsafeObject
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
