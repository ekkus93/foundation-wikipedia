//! Verify declared offline media bytes before pack activation.
//! The caller must independently authenticate the manifest and prove that
//! every required visual was declared. This check only verifies local bytes.

use std::collections::BTreeSet;
use crate::media_objects::{MediaObjectError, MediaObjectStore};

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

/// Rehash every declared object; never trust only a manifest checksum.
pub fn verify_declared_media(
    store: &MediaObjectStore,
    entries: &[RequiredMedia],
) -> Result<u64, MediaInventoryError> {
    if entries.len() > 100_000 {
        return Err(MediaInventoryError::InvalidManifest);
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for entry in entries {
        if entry.digest.len() != 64
            || !entry.digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || entry.bytes == 0
        {
            return Err(MediaInventoryError::InvalidManifest);
        }
        if !seen.insert(&entry.digest) {
            return Err(MediaInventoryError::DuplicateDigest);
        }
        total = total.checked_add(entry.bytes)
            .filter(|n| *n <= (1u64 << 40))
            .ok_or(MediaInventoryError::InvalidManifest)?;
    }
    for entry in entries {
        let bytes = store.read_verified(&entry.digest)
            .map_err(MediaInventoryError::Unreadable)?;
        if bytes.len() as u64 != entry.bytes {
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
    fn inventory_verifies_bytes_and_rejects_corruption() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("wiki-inventory-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"diagram").unwrap();
        let item = RequiredMedia { digest: digest.clone(), bytes: 7 };
        assert_eq!(verify_declared_media(&store, &[item.clone()]).unwrap(), 7);
        assert!(matches!(verify_declared_media(&store, &[item.clone(), item.clone()]),
            Err(MediaInventoryError::DuplicateDigest)));
        assert!(matches!(verify_declared_media(&store, &[RequiredMedia { bytes: 8, ..item.clone() }]),
            Err(MediaInventoryError::WrongSize)));
        fs::write(root.join(&digest[..2]).join(&digest), b"tampered").unwrap();
        assert!(matches!(verify_declared_media(&store, &[item]),
            Err(MediaInventoryError::Unreadable(_))));
        fs::remove_dir_all(root).unwrap();
    }
}
