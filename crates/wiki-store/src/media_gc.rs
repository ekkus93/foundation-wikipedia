//! Read-only, bounded GC candidate preflight for registered media.
//!
//! This does NOT authorize deletion. Installers can write unregistered bytes
//! before committing ownership, and ownership can change after this snapshot.
//! A future collector must coordinate writers, readers, and registry state
//! atomically before deleting any bytes or attribution records.

use crate::media_objects::{MediaObjectError, MediaObjectStore};
use crate::media_ownership::{MediaRegistry, MediaRegistryError};

#[derive(Debug)]
pub enum MediaGcPlanError {
    Registry(MediaRegistryError),
    Store(MediaObjectError),
    SizeMismatch,
    SizeOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaGcCandidate {
    pub digest: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaGcPlan {
    pub candidates: Vec<MediaGcCandidate>,
    pub reclaimable_bytes: u64,
}

/// Return a deterministic advisory list of currently unowned registered
/// objects. Rehash every candidate on disk with bounded memory. Missing,
/// corrupted, symlinked, hardlinked, or size-mismatched objects abort the
/// entire plan; they must never be silently counted as reclaimable.
///
/// This does not enumerate orphan files absent from SQLite and must not be
/// used as a precondition for unlinking objects without a new exclusive
/// ownership check and coordination with concurrent installers/readers.
pub fn plan_unowned_media_gc(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_candidates: usize,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let claims = registry
        .unowned_media_candidates(max_candidates)
        .map_err(MediaGcPlanError::Registry)?;
    let mut candidates = Vec::with_capacity(claims.len());
    let mut reclaimable_bytes = 0u64;
    for (digest, bytes) in claims {
        let verified = store
            .verify_object_size(&digest)
            .map_err(MediaGcPlanError::Store)?;
        if verified != bytes {
            return Err(MediaGcPlanError::SizeMismatch);
        }
        reclaimable_bytes = reclaimable_bytes
            .checked_add(bytes)
            .ok_or(MediaGcPlanError::SizeOverflow)?;
        candidates.push(MediaGcCandidate { digest, bytes });
    }
    Ok(MediaGcPlan {
        candidates,
        reclaimable_bytes,
    })
}

/// Enumerate canonical published media bytes that have no SQLite registry
/// row. This complements the registered-but-unowned planner: a crash between
/// byte publication and ownership registration can leave orphaned objects.
/// The scan is bounded, verifies every reported object's SHA-256, and never
/// unlinks files or changes registry state. Concurrent installers may register
/// an object after the query, so this is NOT safe deletion authorization.
pub fn plan_unregistered_media_gc(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_scanned_objects: usize,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let digests = store
        .published_object_digests(max_scanned_objects)
        .map_err(MediaGcPlanError::Store)?;
    let mut candidates = Vec::new();
    let mut reclaimable_bytes = 0u64;
    for digest in digests {
        if registry
            .registered_digest_size(&digest)
            .map_err(MediaGcPlanError::Registry)?
            .is_some()
        {
            continue;
        }
        let bytes = store
            .verify_object_size(&digest)
            .map_err(MediaGcPlanError::Store)?;
        reclaimable_bytes = reclaimable_bytes
            .checked_add(bytes)
            .ok_or(MediaGcPlanError::SizeOverflow)?;
        candidates.push(MediaGcCandidate { digest, bytes });
    }
    Ok(MediaGcPlan {
        candidates,
        reclaimable_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_ownership::{MediaNotice, OwnerKind};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn setup() -> (std::path::PathBuf, MediaObjectStore, MediaRegistry) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-media-gc-plan-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let registry = MediaRegistry::in_memory().unwrap();
        (root, store, registry)
    }

    fn notice() -> MediaNotice {
        MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Diagram.png".into(),
            creator: "Example creator".into(),
            license: "CC BY-SA 4.0".into(),
        }
    }

    #[test]
    fn only_unowned_registered_bytes_are_reported_without_deletion() {
        let (root, store, mut registry) = setup();
        let shared = store.store_bytes(b"shared diagram").unwrap();
        registry
            .register_verified_bytes(b"shared diagram", &notice())
            .unwrap();
        registry
            .add_owner(&shared, OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .add_owner(&shared, OwnerKind::Cache, "reader")
            .unwrap();
        registry
            .add_owner(&shared, OwnerKind::UserPin, "saved")
            .unwrap();

        let orphan = store.store_bytes(b"unowned plot").unwrap();
        registry
            .register_verified_bytes(b"unowned plot", &notice())
            .unwrap();
        let initial = plan_unowned_media_gc(&store, &registry, 10).unwrap();
        assert_eq!(initial.reclaimable_bytes, 12);
        assert_eq!(
            initial.candidates,
            vec![MediaGcCandidate {
                digest: orphan.clone(),
                bytes: 12,
            }]
        );
        assert_eq!(store.read_verified(&orphan).unwrap(), b"unowned plot");

        registry
            .remove_owner(&shared, OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .remove_owner(&shared, OwnerKind::Cache, "reader")
            .unwrap();
        assert_eq!(
            plan_unowned_media_gc(&store, &registry, 10).unwrap(),
            initial
        );
        registry
            .remove_owner(&shared, OwnerKind::UserPin, "saved")
            .unwrap();
        let all = plan_unowned_media_gc(&store, &registry, 10).unwrap();
        assert_eq!(all.candidates.len(), 2);
        assert_eq!(all.reclaimable_bytes, 26);
        assert!(matches!(
            plan_unowned_media_gc(&store, &registry, 1),
            Err(MediaGcPlanError::Registry(
                MediaRegistryError::TooManyCandidates
            ))
        ));
        assert_eq!(store.read_verified(&shared).unwrap(), b"shared diagram");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unregistered_published_bytes_are_reported_without_touching_registered_media() {
        let (root, store, mut registry) = setup();
        let owned = store.store_bytes(b"owned media").unwrap();
        registry
            .register_verified_bytes(b"owned media", &notice())
            .unwrap();
        registry
            .add_owner(&owned, OwnerKind::Pack, "physics")
            .unwrap();
        let unowned = store.store_bytes(b"registered without owner").unwrap();
        registry
            .register_verified_bytes(b"registered without owner", &notice())
            .unwrap();
        let orphan = store.store_bytes(b"unregistered orphan").unwrap();
        let prefix = root.join(&orphan[..2]);
        fs::write(prefix.join(".123.456.tmp"), b"interrupted staging").unwrap();

        let plan = plan_unregistered_media_gc(&store, &registry, 10).unwrap();
        assert_eq!(
            plan.candidates,
            vec![MediaGcCandidate {
                digest: orphan.clone(),
                bytes: b"unregistered orphan".len() as u64,
            }]
        );
        assert_eq!(plan.reclaimable_bytes, 19);
        assert_eq!(store.read_verified(&owned).unwrap(), b"owned media");
        assert_eq!(
            store.read_verified(&unowned).unwrap(),
            b"registered without owner"
        );
        assert_eq!(
            store.read_verified(&orphan).unwrap(),
            b"unregistered orphan"
        );
        assert_eq!(
            fs::read(prefix.join(".123.456.tmp")).unwrap(),
            b"interrupted staging"
        );
        assert!(matches!(
            plan_unregistered_media_gc(&store, &registry, 2),
            Err(MediaGcPlanError::Store(MediaObjectError::TooManyObjects))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn orphan_scan_rejects_corruption_and_unexpected_paths_without_mutation() {
        let (root, store, registry) = setup();
        let orphan = store.store_bytes(b"orphan diagram").unwrap();
        let prefix = root.join(&orphan[..2]);
        let path = prefix.join(&orphan);
        fs::write(&path, b"corrupt bytes!").unwrap();
        assert!(matches!(
            plan_unregistered_media_gc(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        assert_eq!(fs::read(&path).unwrap(), b"corrupt bytes!");
        fs::write(&path, b"orphan diagram").unwrap();
        fs::write(prefix.join("unexpected"), b"not a digest").unwrap();
        assert!(matches!(
            plan_unregistered_media_gc(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::UnsafeObject))
        ));
        fs::remove_file(prefix.join("unexpected")).unwrap();
        assert_eq!(
            plan_unregistered_media_gc(&store, &registry, 10)
                .unwrap()
                .candidates
                .len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn orphan_scan_rejects_symlinked_prefix_and_published_digest() {
        use std::os::unix::fs::symlink;
        let (root, store, registry) = setup();
        let orphan = store.store_bytes(b"unregistered figure").unwrap();
        let path = root.join(&orphan[..2]).join(&orphan);
        fs::remove_file(&path).unwrap();
        symlink(root.join("missing"), &path).unwrap();
        assert!(matches!(
            plan_unregistered_media_gc(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::UnsafeObject))
        ));
        fs::remove_file(&path).unwrap();
        let prefix = root.join(&orphan[..2]);
        let target = root.with_extension("outside");
        fs::create_dir(&target).unwrap();
        fs::remove_dir(&prefix).unwrap();
        symlink(&target, &prefix).unwrap();
        assert!(matches!(
            plan_unregistered_media_gc(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::UnsafeDirectory))
        ));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn missing_or_corrupt_candidates_fail_closed_and_leave_bytes_untouched() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"plot").unwrap();
        registry
            .register_verified_bytes(b"plot", &notice())
            .unwrap();
        assert!(matches!(
            plan_unowned_media_gc(&store, &registry, 0),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert!(matches!(
            plan_unowned_media_gc(&store, &registry, 10_001),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        let path = root.join(&digest[..2]).join(&digest);
        fs::write(&path, b"evil").unwrap();
        assert!(matches!(
            plan_unowned_media_gc(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        assert_eq!(fs::read(&path).unwrap(), b"evil");
        fs::remove_file(&path).unwrap();
        assert!(matches!(
            plan_unowned_media_gc(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::Io(_)))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
