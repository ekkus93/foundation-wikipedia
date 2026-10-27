//! Read-only, bounded GC candidate preflight for registered media.
//!
//! This does NOT authorize deletion. Installers can write unregistered bytes
//! before committing ownership, and ownership can change after this snapshot.
//! A future collector must coordinate writers, readers, and registry state
//! atomically before deleting any bytes or attribution records.

use crate::media_lease::{MediaLeaseCoordinator, MediaLeaseError};
use crate::media_objects::{MediaObjectError, MediaObjectStore};
use crate::media_ownership::{MediaRegistry, MediaRegistryError};

#[derive(Debug)]
pub enum MediaGcPlanError {
    Registry(MediaRegistryError),
    Lease(MediaLeaseError),
    Store(MediaObjectError),
    SizeMismatch,
    SizeOverflow,
    PostCommitDeletion {
        source: MediaObjectError,
        completed_objects: usize,
    },
    PostCommitSizeMismatch,
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
    verify_unowned_candidates(store, &claims, false)
}

/// Revalidate a bounded SQLite snapshot while it remains protected by a
/// registry writer reservation. The reservation excludes competing SQLite
/// owner inserts, but this remains advisory until installers and file readers
/// participate in a shared filesystem lease and crash-recovery protocol.
pub fn plan_unowned_media_gc_with_registry_reservation(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_candidates: usize,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    // Lock order: exclusive filesystem lease before the registry IMMEDIATE
    // reservation. Cooperating installers take their shared lease first.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    registry
        .with_reserved_unowned_media(max_candidates, |claims| {
            verify_unowned_candidates(store, claims, true)
        })
        .map_err(MediaGcPlanError::Registry)?
}

fn verify_unowned_candidates(
    store: &MediaObjectStore,
    claims: &[(String, u64)],
    exclusive_lease_held: bool,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let mut candidates = Vec::with_capacity(claims.len());
    let mut reclaimable_bytes = 0u64;
    for (digest, bytes) in claims {
        let verified = if exclusive_lease_held {
            store.verify_object_size_unlocked(digest)
        } else {
            store.verify_object_size(digest)
        }
        .map_err(MediaGcPlanError::Store)?;
        if verified != *bytes {
            return Err(MediaGcPlanError::SizeMismatch);
        }
        reclaimable_bytes = reclaimable_bytes
            .checked_add(*bytes)
            .ok_or(MediaGcPlanError::SizeOverflow)?;
        candidates.push(MediaGcCandidate {
            digest: digest.clone(),
            bytes: *bytes,
        });
    }
    Ok(MediaGcPlan {
        candidates,
        reclaimable_bytes,
    })
}

/// Completed media reclamation; only counts bytes after durable filesystem
/// unlink and parent-directory sync. No owner or pinned content is eligible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaGcSweep {
    pub removed_objects: usize,
    pub reclaimed_bytes: u64,
}

/// Commit removal of only currently unowned registered media, then unlink
/// their bytes while the EXCLUSIVE cross-process media lease remains held.
/// The SQLite commit always precedes unlink, so an interruption may leave
/// unregistered byte orphans but cannot strand an owner with missing media.
/// The existing orphan audit can identify such leftovers for later recovery.
///
/// All cooperating installers and readers must use their SHARED media lease.
/// External non-cooperating filesystem writers and malicious directory
/// replacement are beyond this lease protocol; do not expose a public
/// untrusted storage root or treat this as publisher provenance verification.
pub fn collect_unowned_registered_media(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_candidates: usize,
) -> Result<MediaGcSweep, MediaGcPlanError> {
    collect_unowned_registered_media_with_unlink(store, registry, max_candidates, |digest| {
        store.unlink_verified_unlocked(digest)
    })
}

fn collect_unowned_registered_media_with_unlink(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_candidates: usize,
    mut unlink: impl FnMut(&str) -> Result<u64, MediaObjectError>,
) -> Result<MediaGcSweep, MediaGcPlanError> {
    // Lock order must be filesystem lease first, SQLite writer second.
    // Keep the exclusive lease until every committed deletion is durable.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    let (_, candidates) = registry
        .commit_verified_unowned_media(max_candidates, |claims| {
            verify_unowned_candidates(store, claims, true)
        })
        .map_err(MediaGcPlanError::Registry)??;

    let mut report = MediaGcSweep {
        removed_objects: 0,
        reclaimed_bytes: 0,
    };
    for (digest, bytes) in candidates {
        // A filesystem error here happens AFTER SQLite commit. The
        // unlinked remainder is now an unregistered orphan, which is
        // discoverable by the existing orphan inventory audit.
        let removed = unlink(&digest).map_err(|source| MediaGcPlanError::PostCommitDeletion {
            source,
            completed_objects: report.removed_objects,
        })?;
        if removed != bytes {
            return Err(MediaGcPlanError::PostCommitSizeMismatch);
        }
        report.removed_objects += 1;
        report.reclaimed_bytes = report
            .reclaimed_bytes
            .checked_add(removed)
            .ok_or(MediaGcPlanError::SizeOverflow)?;
    }
    Ok(report)
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

/// Recover disk space from unregistered, fully published objects left by
/// interrupted installs or a SQLite-first GC crash. This path never deletes
/// a registered object, even if that object currently has zero owners.
///
/// Under the EXCLUSIVE media lease, enumerate canonical published objects,
/// acquire SQLite's IMMEDIATE writer reservation, identify absent rows,
/// verify ALL candidate hashes first, then unlink/sync those files before
/// releasing the registry reservation. A crash midway may leave remaining
/// orphans but never dangling SQLite owner records. Temporary publication
/// staging files are deliberately left untouched.
pub fn collect_unregistered_media_orphans(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_scanned_objects: usize,
) -> Result<MediaGcSweep, MediaGcPlanError> {
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    let digests = store
        .published_object_digests_unlocked(max_scanned_objects)
        .map_err(MediaGcPlanError::Store)?;
    registry
        .with_reserved_unregistered_media(&digests, |absent| {
            // Ensure a corrupt or unsafe orphan rejects the entire pass
            // before deleting any candidate from this bounded snapshot.
            let mut sizes = Vec::with_capacity(absent.len());
            for digest in absent {
                let bytes = store
                    .verify_object_size_unlocked(digest)
                    .map_err(MediaGcPlanError::Store)?;
                sizes.push((digest, bytes));
            }
            let mut report = MediaGcSweep {
                removed_objects: 0,
                reclaimed_bytes: 0,
            };
            for (digest, expected) in sizes {
                let bytes = store
                    .unlink_verified_unlocked(digest)
                    .map_err(MediaGcPlanError::Store)?;
                if bytes != expected {
                    return Err(MediaGcPlanError::SizeMismatch);
                }
                report.removed_objects += 1;
                report.reclaimed_bytes = report
                    .reclaimed_bytes
                    .checked_add(bytes)
                    .ok_or(MediaGcPlanError::SizeOverflow)?;
            }
            Ok(report)
        })
        .map_err(MediaGcPlanError::Registry)?
}

/// A read-only integrity audit of *every* registered media object, including
/// actively pack-owned, cached and user-pinned assets. Unowned-only GC
/// preflights cannot detect damaged media that is still needed for reading.
/// Verify each digest and declared size through the bounded streaming reader.
/// Also reject invalid ownership, dangling references and conflicting MIME claims.
/// Also require at least one valid, bounded attribution notice for each object,
/// including currently unowned bytes. Missing/corrupt/unsafe objects or
/// malformed/missing attribution abort the audit rather than being skipped.
///
/// The registry is sampled before filesystem reads. Concurrent writes may
/// change its state, so success is not a transactional pack-activation proof,
/// an official-source authenticity claim, or permission to garbage-collect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredMediaAudit {
    pub objects: usize,
    pub verified_bytes: u64,
}

pub fn audit_registered_media_integrity(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_objects: usize,
) -> Result<RegisteredMediaAudit, MediaGcPlanError> {
    registry
        .audit_unregistered_references()
        .map_err(MediaGcPlanError::Registry)?;
    let claims = registry
        .registered_media_candidates(max_objects)
        .map_err(MediaGcPlanError::Registry)?;
    let mut verified_bytes = 0u64;
    for (digest, declared_bytes) in &claims {
        registry
            .audit_registered_notices(digest, 1024)
            .map_err(MediaGcPlanError::Registry)?;
        registry
            .audit_registered_owners(digest, 1024)
            .map_err(MediaGcPlanError::Registry)?;
        let actual_bytes = store
            .verify_object_size(digest)
            .map_err(MediaGcPlanError::Store)?;
        if actual_bytes != *declared_bytes {
            return Err(MediaGcPlanError::SizeMismatch);
        }
        verified_bytes = verified_bytes
            .checked_add(actual_bytes)
            .ok_or(MediaGcPlanError::SizeOverflow)?;
    }
    Ok(RegisteredMediaAudit {
        objects: claims.len(),
        verified_bytes,
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
    fn orphan_collector_reclaims_only_unregistered_bytes_and_keeps_staging() {
        let (root, store, mut registry) = setup();
        let owned = store.store_bytes(b"owned chart").unwrap();
        registry
            .register_verified_bytes(b"owned chart", &notice())
            .unwrap();
        registry.add_owner(&owned, OwnerKind::Pack, "math").unwrap();
        let registered_unowned = store.store_bytes(b"not yet owned").unwrap();
        registry
            .register_verified_bytes(b"not yet owned", &notice())
            .unwrap();
        let orphan = store.store_bytes(b"interrupted install").unwrap();
        let staging = root.join(&orphan[..2]).join(".123.456.tmp");
        fs::write(&staging, b"partial staging").unwrap();

        let sweep = collect_unregistered_media_orphans(&store, &mut registry, 10).unwrap();
        assert_eq!(sweep.removed_objects, 1);
        assert_eq!(sweep.reclaimed_bytes, b"interrupted install".len() as u64);
        assert!(matches!(
            store.read_verified(&orphan),
            Err(MediaObjectError::Io(_))
        ));
        assert_eq!(store.read_verified(&owned).unwrap(), b"owned chart");
        assert_eq!(
            store.read_verified(&registered_unowned).unwrap(),
            b"not yet owned"
        );
        assert_eq!(fs::read(staging).unwrap(), b"partial staging");
        assert_eq!(
            collect_unregistered_media_orphans(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn orphan_collector_rejects_corruption_without_partial_deletion() {
        let (root, store, mut registry) = setup();
        let first = store.store_bytes(b"orphan A").unwrap();
        let second = store.store_bytes(b"orphan B").unwrap();
        fs::write(root.join(&second[..2]).join(&second), b"corrupt!").unwrap();
        assert!(matches!(
            collect_unregistered_media_orphans(&store, &mut registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        assert_eq!(store.read_verified(&first).unwrap(), b"orphan A");
        assert!(root.join(&second[..2]).join(&second).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_gc_reclaims_only_unowned_and_preserves_shared_pins() {
        let (root, store, mut registry) = setup();
        let kept = store.store_bytes(b"pinned image").unwrap();
        registry
            .register_verified_bytes(b"pinned image", &notice())
            .unwrap();
        registry
            .add_owner(&kept, OwnerKind::Pack, "biology")
            .unwrap();
        registry
            .add_owner(&kept, OwnerKind::UserPin, "saved")
            .unwrap();
        let released = store.store_bytes(b"released image").unwrap();
        registry
            .register_verified_bytes(b"released image", &notice())
            .unwrap();
        let before = plan_unowned_media_gc(&store, &registry, 10).unwrap();
        assert_eq!(before.candidates.len(), 1);
        assert_eq!(before.candidates[0].digest, released);

        let sweep = collect_unowned_registered_media(&store, &mut registry, 10).unwrap();
        assert_eq!(sweep.removed_objects, 1);
        assert_eq!(sweep.reclaimed_bytes, b"released image".len() as u64);
        assert!(registry
            .registered_digest_size(&released)
            .unwrap()
            .is_none());
        assert!(matches!(
            store.read_verified(&released),
            Err(MediaObjectError::Io(_))
        ));
        assert_eq!(store.read_verified(&kept).unwrap(), b"pinned image");
        assert_eq!(registry.notice_count(&kept).unwrap(), 1);
        assert_eq!(
            collect_unowned_registered_media(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            0
        );
        registry
            .remove_owner(&kept, OwnerKind::Pack, "biology")
            .unwrap();
        assert_eq!(
            collect_unowned_registered_media(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            0
        );
        registry
            .remove_owner(&kept, OwnerKind::UserPin, "saved")
            .unwrap();
        assert_eq!(
            collect_unowned_registered_media(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_gc_corrupt_candidate_rolls_back_all_registry_deletes() {
        let (root, store, mut registry) = setup();
        for bytes in [b"first plot".as_slice(), b"second plot".as_slice()] {
            let digest = store.store_bytes(bytes).unwrap();
            registry.register_verified_bytes(bytes, &notice()).unwrap();
            if bytes == b"second plot" {
                fs::write(root.join(&digest[..2]).join(&digest), b"corrupt plot").unwrap();
            }
        }
        assert!(matches!(
            collect_unowned_registered_media(&store, &mut registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        assert_eq!(registry.unowned_media_candidates(10).unwrap().len(), 2);
        assert_eq!(registry.registered_media_candidates(10).unwrap().len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_after_registry_commit_leaves_safe_orphan_for_recovery() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"crash window").unwrap();
        registry
            .register_verified_bytes(b"crash window", &notice())
            .unwrap();
        let result =
            collect_unowned_registered_media_with_unlink(&store, &mut registry, 10, |_| {
                Err(MediaObjectError::UnsafeObject)
            });
        assert!(matches!(
            result,
            Err(MediaGcPlanError::PostCommitDeletion {
                completed_objects: 0,
                ..
            })
        ));
        assert_eq!(registry.registered_digest_size(&digest).unwrap(), None);
        assert_eq!(store.read_verified(&digest).unwrap(), b"crash window");
        let orphan = plan_unregistered_media_gc(&store, &registry, 10).unwrap();
        assert_eq!(orphan.candidates[0].digest, digest);
        let recovered = collect_unregistered_media_orphans(&store, &mut registry, 10).unwrap();
        assert_eq!(recovered.removed_objects, 1);
        assert_eq!(recovered.reclaimed_bytes, b"crash window".len() as u64);
        assert_eq!(
            collect_unregistered_media_orphans(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reserved_gc_planner_is_read_only_and_releases_sqlite_writer_on_error() {
        let (root, store, _) = setup();
        let database = root.join("registry.sqlite");
        let mut registry = MediaRegistry::open(&database).unwrap();
        let digest = store.store_bytes(b"unowned plan").unwrap();
        registry
            .register_verified_bytes(b"unowned plan", &notice())
            .unwrap();
        let plan =
            plan_unowned_media_gc_with_registry_reservation(&store, &mut registry, 10).unwrap();
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].digest, digest);
        assert_eq!(plan.reclaimable_bytes, b"unowned plan".len() as u64);
        assert_eq!(registry.registered_digest_size(&digest).unwrap(), Some(12));

        // A failed digest verification also releases the reservation. It
        // cannot delete an object or its attribution/ownership metadata.
        fs::write(root.join(&digest[..2]).join(&digest), b"corrupted...").unwrap();
        assert!(matches!(
            plan_unowned_media_gc_with_registry_reservation(&store, &mut registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        registry
            .add_owner(&digest, OwnerKind::UserPin, "reader")
            .unwrap();
        assert!(
            plan_unowned_media_gc_with_registry_reservation(&store, &mut registry, 10)
                .unwrap()
                .candidates
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_integrity_audit_covers_owned_and_unowned_but_not_orphaned_bytes() {
        let (root, store, mut registry) = setup();
        let owned = store.store_bytes(b"owned diagram").unwrap();
        registry
            .register_verified_bytes(b"owned diagram", &notice())
            .unwrap();
        registry
            .add_owner(&owned, OwnerKind::Pack, "physics")
            .unwrap();
        let unowned = store.store_bytes(b"unowned plot").unwrap();
        registry
            .register_verified_bytes(b"unowned plot", &notice())
            .unwrap();
        let orphan = store.store_bytes(b"unregistered figure").unwrap();

        let audit = audit_registered_media_integrity(&store, &registry, 10).unwrap();
        assert_eq!(
            audit,
            RegisteredMediaAudit {
                objects: 2,
                verified_bytes: (b"owned diagram".len() + b"unowned plot".len()) as u64,
            }
        );
        assert_eq!(
            plan_unregistered_media_gc(&store, &registry, 10)
                .unwrap()
                .candidates[0]
                .digest,
            orphan
        );
        assert_eq!(store.read_verified(&unowned).unwrap(), b"unowned plot");

        let owned_path = root.join(&owned[..2]).join(&owned);
        fs::write(&owned_path, b"bad!! diagram").unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        fs::write(&owned_path, b"owned diagram").unwrap();
        fs::remove_file(&owned_path).unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::Io(_)))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_integrity_audit_requires_valid_bounded_attribution() {
        let (root, store, _) = setup();
        let db = root.join("attribution.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        let digest = store.store_bytes(b"attributed visual").unwrap();
        registry
            .register_verified_bytes(b"attributed visual", &notice())
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Pack, "science")
            .unwrap();
        assert_eq!(
            audit_registered_media_integrity(&store, &registry, 10)
                .unwrap()
                .objects,
            1
        );

        let mut raw = rusqlite::Connection::open(&db).unwrap();
        raw.execute(
            "UPDATE media_notices SET creator='bad' || char(10) WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        raw.execute(
            "DELETE FROM media_notices WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));

        registry
            .register_verified_bytes(b"attributed visual", &notice())
            .unwrap();
        let tx = raw.transaction().unwrap();
        for index in 0..1024 {
            tx.execute(
                "INSERT INTO media_notices(digest,mime,source_url,creator,license)
                 VALUES (?1,'image/png','https://commons.wikimedia.org/wiki/File:Diagram.png',?2,'CC BY-SA 4.0')",
                rusqlite::params![digest, format!("Contributor {index}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(
                MediaRegistryError::TooManyNotices
            ))
        ));
        raw.execute(
            "DELETE FROM media_notices WHERE creator LIKE 'Contributor %'",
            [],
        )
        .unwrap();
        assert_eq!(
            audit_registered_media_integrity(&store, &registry, 10)
                .unwrap()
                .verified_bytes,
            b"attributed visual".len() as u64
        );
        drop(raw);
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_integrity_audit_rejects_corrupt_owner_and_dangling_rows() {
        let (root, store, _) = setup();
        let db = root.join("owners.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        let digest = store.store_bytes(b"owned visual").unwrap();
        registry
            .register_verified_bytes(b"owned visual", &notice())
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Pack, "science")
            .unwrap();
        let raw = rusqlite::Connection::open(&db).unwrap();
        raw.execute(
            "UPDATE media_owners SET owner_id='bad/owner' WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        raw.execute(
            "UPDATE media_owners SET owner_id='science' WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        // Simulate a damaged database written without foreign-key enforcement.
        raw.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        let orphan = "0".repeat(64);
        raw.execute(
            "INSERT INTO media_owners(digest,kind,owner_id) VALUES (?1,'pack','ghost')",
            rusqlite::params![orphan],
        )
        .unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        raw.execute(
            "DELETE FROM media_owners WHERE digest=?1",
            rusqlite::params![orphan],
        )
        .unwrap();
        raw.execute(
            "INSERT INTO media_notices(digest,mime,source_url,creator,license)
             VALUES (?1,'image/png','https://commons.wikimedia.org/wiki/File:Orphan.png',
                     'Creator','CC BY-SA 4.0')",
            rusqlite::params![orphan],
        )
        .unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        raw.execute(
            "DELETE FROM media_notices WHERE digest=?1",
            rusqlite::params![orphan],
        )
        .unwrap();
        assert_eq!(
            audit_registered_media_integrity(&store, &registry, 10)
                .unwrap()
                .objects,
            1
        );
        drop(raw);
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_integrity_audit_rejects_mime_conflict_and_owner_fanout() {
        let (root, store, _) = setup();
        let db = root.join("fanout.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        let digest = store.store_bytes(b"shared visual").unwrap();
        registry
            .register_verified_bytes(b"shared visual", &notice())
            .unwrap();
        let mut second = notice();
        second.creator = "Second creator".into();
        registry
            .register_verified_bytes(b"shared visual", &second)
            .unwrap();
        // Simulate a damaged historical database; public registration now
        // rejects conflicting MIME before committing any attribution.
        let mut raw = rusqlite::Connection::open(&db).unwrap();
        raw.execute(
            "UPDATE media_notices SET mime='image/svg+xml'
             WHERE digest=?1 AND creator='Second creator'",
            rusqlite::params![digest],
        )
        .unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(
                MediaRegistryError::ConflictingMime
            ))
        ));
        raw.execute(
            "DELETE FROM media_notices WHERE digest=?1 AND mime='image/svg+xml'",
            rusqlite::params![digest],
        )
        .unwrap();
        let tx = raw.transaction().unwrap();
        for i in 0..1025 {
            tx.execute(
                "INSERT INTO media_owners(digest,kind,owner_id) VALUES (?1,'pack',?2)",
                rusqlite::params![digest, format!("pack_{i}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 10),
            Err(MediaGcPlanError::Registry(
                MediaRegistryError::TooManyOwners
            ))
        ));
        raw.execute("DELETE FROM media_owners WHERE owner_id='pack_1024'", [])
            .unwrap();
        assert_eq!(
            audit_registered_media_integrity(&store, &registry, 10)
                .unwrap()
                .objects,
            1
        );
        drop(raw);
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registered_integrity_audit_is_bounded_even_if_all_objects_are_owned() {
        let (root, store, mut registry) = setup();
        for (name, content) in [("one", b"one".as_slice()), ("two", b"two".as_slice())] {
            let digest = store.store_bytes(content).unwrap();
            registry
                .register_verified_bytes(content, &notice())
                .unwrap();
            registry
                .add_owner(&digest, OwnerKind::UserPin, name)
                .unwrap();
        }
        assert!(matches!(
            audit_registered_media_integrity(&store, &registry, 1),
            Err(MediaGcPlanError::Registry(
                MediaRegistryError::TooManyCandidates
            ))
        ));
        for limit in [0, 10_001] {
            assert!(matches!(
                audit_registered_media_integrity(&store, &registry, limit),
                Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
            ));
        }
        assert_eq!(
            audit_registered_media_integrity(&store, &registry, 2)
                .unwrap()
                .verified_bytes,
            6
        );
        fs::remove_dir_all(root).unwrap();
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
