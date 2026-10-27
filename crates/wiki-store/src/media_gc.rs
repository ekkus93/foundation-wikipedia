//! Bounded media audits, advisory GC planning and lease-coordinated reclamation.
//!
//! Advisory plans are never deletion authority. Production collectors hold
//! an EXCLUSIVE media lease and SQLite writer reservation, verify candidates,
//! commit removal of unowned registry rows before unlinking, and fsync the
//! containing directories. All readers/installers must cooperate with the
//! SHARED lease protocol; hostile direct filesystem/SQLite writes are not
//! protected by this advisory coordination.

use crate::media_lease::{MediaLeaseCoordinator, MediaLeaseError};
use crate::media_objects::{MediaObjectError, MediaObjectStore};
use crate::media_ownership::{MediaRegistry, MediaRegistryError, OwnerKind};

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
    /// An unregistered published object could belong to a different registry.
    /// Never bind while any such file remains unexplained.
    UnregisteredOrphans {
        count: usize,
    },
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
/// Hold one SHARED media lease from SQLite inventory lookup through the
/// entire streaming verification pass. This excludes cooperating collectors,
/// but does not freeze direct SQLite owner changes or authorize deletion.
/// Unregistered orphan files are not included in this advisory result.
pub fn plan_unowned_media_gc(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_candidates: usize,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaGcPlanError::Lease)?;
    let claims = registry
        .unowned_media_candidates(max_candidates)
        .map_err(MediaGcPlanError::Registry)?;
    verify_unowned_candidates(store, &claims, true)
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
    lease_held: bool,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let mut candidates = Vec::with_capacity(claims.len());
    let mut reclaimable_bytes = 0u64;
    for (digest, bytes) in claims {
        let verified = if lease_held {
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
    collect_unowned_registered_media_under_lease(store, registry, max_candidates, &mut unlink)
}

/// Internal collector assumes the caller already owns the EXCLUSIVE lease.
/// Never call public leasing methods while inside this scope: nested SQLite
/// lock acquisition could deadlock behind a waiting external GC.
fn collect_unowned_registered_media_under_lease(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_candidates: usize,
    unlink: &mut impl FnMut(&str) -> Result<u64, MediaObjectError>,
) -> Result<MediaGcSweep, MediaGcPlanError> {
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

/// Owner release committed under an exclusive media lease. A cleanup error
/// is reported separately because it cannot roll back a successful owner
/// release: callers must not retry an uninstall as though the owner remained.
#[derive(Debug)]
pub struct OwnerReleaseGcOutcome {
    pub released_objects: usize,
    pub cleanup: Result<MediaGcSweep, MediaGcPlanError>,
}

/// Coordinate removing a pack, cache entry or user pin with a bounded media
/// reclamation pass. One EXCLUSIVE lease spans the SQLite owner release and
/// registered-object GC; shared-lease readers/installers cannot observe an
/// intervening GC state. The sweep rechecks all owners inside an IMMEDIATE
/// writer transaction: a shared digest is retained until its final owner
/// releases it.
///
/// The owner release commits before cleanup. The returned outer Err means
/// nothing was released. After a successful release, the inner cleanup Err
/// means unowned bytes/metadata may remain and GC can be retried separately.
/// No stale digest list is ever passed directly to filesystem deletion.
pub fn release_owner_and_reclaim_media(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    max_candidates: usize,
) -> Result<OwnerReleaseGcOutcome, MediaGcPlanError> {
    release_owner_and_reclaim_media_with_binding(
        store,
        registry,
        kind,
        owner_id,
        max_candidates,
        false,
    )
}

/// Root-bound owner removal for production pack/cache/pin lifecycle callers.
/// Check the persistent registry binding under the SAME exclusive lease that
/// protects owner release and the subsequent reclaim pass. A mismatched or
/// unbound registry must never revoke owners or delete files through this API.
/// The legacy low-level release API remains available for in-memory fixtures.
pub fn release_bound_owner_and_reclaim_media(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    max_candidates: usize,
) -> Result<OwnerReleaseGcOutcome, MediaGcPlanError> {
    release_owner_and_reclaim_media_with_binding(
        store,
        registry,
        kind,
        owner_id,
        max_candidates,
        true,
    )
}

fn release_owner_and_reclaim_media_with_binding(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    max_candidates: usize,
    require_binding: bool,
) -> Result<OwnerReleaseGcOutcome, MediaGcPlanError> {
    // Fail invalid sweep bounds *before* mutating the owner's inventory.
    if !(1..=10_000).contains(&max_candidates) {
        return Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput));
    }
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaGcPlanError::Registry)?;
    }
    let released = registry
        .remove_all_for_owner(kind, owner_id)
        .map_err(MediaGcPlanError::Registry)?;
    let cleanup = collect_unowned_registered_media_under_lease(
        store,
        registry,
        max_candidates,
        &mut |digest| store.unlink_verified_unlocked(digest),
    );
    Ok(OwnerReleaseGcOutcome {
        released_objects: released.len(),
        cleanup,
    })
}

/// Result of one bounded maintenance pass. Each phase reports its own
/// outcome because a registered-media failure may happen after SQLite commit
/// and the orphan sweep may still recover its leftover published bytes.
#[derive(Debug)]
pub struct MediaMaintenanceOutcome {
    pub registered: Result<MediaGcSweep, MediaGcPlanError>,
    pub orphans: Result<MediaGcSweep, MediaGcPlanError>,
}

/// Run both registered-unowned reclamation and orphan recovery under ONE
/// exclusive media lease. Callers can invoke this on startup or from a
/// maintenance job; this does not itself install a timer or background task.
///
/// Validate both budgets before acquiring the lease or modifying the
/// registry. The registered phase commits SQLite removal before unlinking.
/// Even if that phase fails, run the orphan phase: it can reclaim files left
/// by an interrupted registered deletion without risking any owned media.
/// Always inspect both phase results; an outer Ok is not proof both passed.
pub fn run_bounded_media_maintenance(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered_candidates: usize,
    max_scanned_objects: usize,
) -> Result<MediaMaintenanceOutcome, MediaGcPlanError> {
    run_bounded_media_maintenance_with_unlink(
        store,
        registry,
        max_registered_candidates,
        max_scanned_objects,
        |digest| store.unlink_verified_unlocked(digest),
    )
}

/// Production maintenance boundary for a persistently root-bound registry.
/// Recheck the binding AFTER taking the exclusive media lease and BEFORE
/// committing any SQLite deletions. The CLI's earlier binding check is only
/// an advisory preflight; it cannot replace this lock-scoped check.
/// This still cannot prevent uncooperative direct SQLite/filesystem mutation.
pub fn run_bound_media_maintenance(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered_candidates: usize,
    max_scanned_objects: usize,
) -> Result<MediaMaintenanceOutcome, MediaGcPlanError> {
    run_bounded_media_maintenance_with_unlink_and_binding(
        store,
        registry,
        max_registered_candidates,
        max_scanned_objects,
        true,
        |digest| store.unlink_verified_unlocked(digest),
    )
}

/// Result of an opt-in periodic maintenance invocation. The external timer
/// (cron/systemd/app job) invokes the CLI; the persisted success watermark
/// makes repeated invocations safe and avoids duplicate sweeps.
#[derive(Debug)]
pub enum ScheduledMediaMaintenance {
    NotDue { next_due_epoch_seconds: u64 },
    Ran {
        maintenance: MediaMaintenanceOutcome,
        /// Ok(true) only when both phases succeeded and the durable success
        /// timestamp was committed. Ok(false) means cleanup was incomplete
        /// and should be retried; Err means cleanup passed but the checkpoint
        /// write failed, so the next invocation may repeat the empty sweep.
        checkpoint: Result<bool, MediaRegistryError>,
    },
}

/// Run a bounded, explicitly authorized cleanup only when its last *fully
/// successful* pass is older than the interval. The exclusive media lease
/// protects the bound-root check, persisted due check, both cleanup phases
/// and success checkpoint from cooperating concurrent scheduled invocations.
/// A failed/partial cleanup never advances the success watermark.
///
/// This function does not start a daemon, authenticate a publisher, or defend
/// against hostile unleased registry/filesystem writers. Callers must ensure
/// private directory permissions and invoke it via an opt-in host scheduler.
pub fn run_bound_media_maintenance_if_due(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered_candidates: usize,
    max_scanned_objects: usize,
    interval_seconds: u64,
    now_epoch_seconds: u64,
) -> Result<ScheduledMediaMaintenance, MediaGcPlanError> {
    if !(1..=10_000).contains(&max_registered_candidates)
        || !(1..=10_000).contains(&max_scanned_objects)
        || !(60..=2_592_000).contains(&interval_seconds)
        || now_epoch_seconds == 0
        || now_epoch_seconds > i64::MAX as u64
    {
        return Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput));
    }
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    registry
        .require_media_root_binding(store.root())
        .map_err(MediaGcPlanError::Registry)?;
    if let Some(last) = registry
        .media_maintenance_last_success()
        .map_err(MediaGcPlanError::Registry)?
    {
        let next_due_epoch_seconds = last.saturating_add(interval_seconds);
        if now_epoch_seconds < next_due_epoch_seconds {
            return Ok(ScheduledMediaMaintenance::NotDue {
                next_due_epoch_seconds,
            });
        }
    }
    let maintenance = run_maintenance_phases_under_lease(
        store,
        registry,
        max_registered_candidates,
        max_scanned_objects,
        &mut |digest| store.unlink_verified_unlocked(digest),
    );
    let checkpoint = if maintenance.registered.is_ok() && maintenance.orphans.is_ok() {
        registry
            .mark_media_maintenance_success(now_epoch_seconds)
            .map(|()| true)
    } else {
        Ok(false)
    };
    Ok(ScheduledMediaMaintenance::Ran {
        maintenance,
        checkpoint,
    })
}

fn run_bounded_media_maintenance_with_unlink(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered_candidates: usize,
    max_scanned_objects: usize,
    unlink: impl FnMut(&str) -> Result<u64, MediaObjectError>,
) -> Result<MediaMaintenanceOutcome, MediaGcPlanError> {
    run_bounded_media_maintenance_with_unlink_and_binding(
        store,
        registry,
        max_registered_candidates,
        max_scanned_objects,
        false,
        unlink,
    )
}

fn run_bounded_media_maintenance_with_unlink_and_binding(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered_candidates: usize,
    max_scanned_objects: usize,
    require_binding: bool,
    mut unlink: impl FnMut(&str) -> Result<u64, MediaObjectError>,
) -> Result<MediaMaintenanceOutcome, MediaGcPlanError> {
    if !(1..=10_000).contains(&max_registered_candidates)
        || !(1..=10_000).contains(&max_scanned_objects)
    {
        return Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput));
    }
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaGcPlanError::Registry)?;
    }
    Ok(run_maintenance_phases_under_lease(
        store,
        registry,
        max_registered_candidates,
        max_scanned_objects,
        &mut unlink,
    ))
}

/// Both GC phases under an already-held EXCLUSIVE media lease. Never take a
/// nested SQLite-backed lease here: a waiting writer could deadlock it.
fn run_maintenance_phases_under_lease(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered_candidates: usize,
    max_scanned_objects: usize,
    unlink: &mut impl FnMut(&str) -> Result<u64, MediaObjectError>,
) -> MediaMaintenanceOutcome {
    let registered = collect_unowned_registered_media_under_lease(
        store,
        registry,
        max_registered_candidates,
        unlink,
    );
    let orphans =
        collect_unregistered_media_orphans_under_lease(store, registry, max_scanned_objects);
    MediaMaintenanceOutcome {
        registered,
        orphans,
    }
}

/// Enumerate canonical published media bytes that have no SQLite registry
/// row. This complements the registered-but-unowned planner: a crash between
/// byte publication and ownership registration can leave orphaned objects.
/// The scan is bounded, verifies every reported object's SHA-256, and never
/// unlinks files or changes registry state. A single SHARED lease covers the
/// directory scan, registry lookups and all file verification, excluding
/// cooperating collectors. Direct registry writers can still change owners;
/// this is NOT safe deletion authorization.
pub fn plan_unregistered_media_gc(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_scanned_objects: usize,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaGcPlanError::Lease)?;
    plan_unregistered_media_gc_under_lease(store, registry, max_scanned_objects)
}

/// Read-only orphan audit when a SHARED or EXCLUSIVE lease is already held.
/// Avoid nested lease acquisition while an exclusive binder is running.
fn plan_unregistered_media_gc_under_lease(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_scanned_objects: usize,
) -> Result<MediaGcPlan, MediaGcPlanError> {
    let digests = store
        .published_object_digests_unlocked(max_scanned_objects)
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
            .verify_object_size_unlocked(&digest)
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
    collect_unregistered_media_orphans_under_lease(store, registry, max_scanned_objects)
}

/// Internal orphan collector assumes an EXCLUSIVE media lease is held.
fn collect_unregistered_media_orphans_under_lease(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_scanned_objects: usize,
) -> Result<MediaGcSweep, MediaGcPlanError> {
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
/// A single SHARED media lease covers the registry inventory, attribution
/// audit and every streaming digest verification, preventing a cooperating
/// EXCLUSIVE collector from deleting media between snapshot and file reads.
/// Direct SQLite writers can still change registry rows: this is not an
/// immutable ownership snapshot, publisher authentication, pack activation
/// or permission to garbage-collect.
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
    // Acquire before sampling SQLite and retain through all file reads.
    // Calling the public leased verify_object_size from this scope could
    // deadlock behind a queued EXCLUSIVE collector.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaGcPlanError::Lease)?;
    audit_registered_media_integrity_under_lease(store, registry, max_objects)
}

/// Complete read-only registered-object audit under a caller-held media lease.
fn audit_registered_media_integrity_under_lease(
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
            .verify_object_size_unlocked(digest)
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

/// Read-only pre-binding diagnostics for an existing ownership registry.
/// The registered inventory is verified completely and every published
/// unregistered object is SHA-256 checked. No binding or deletion is performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaBindingPreflight {
    pub registered: RegisteredMediaAudit,
    pub orphans: MediaGcPlan,
}

/// Allow an operator to reconcile unbound registries with orphan files
/// without first granting the registry deletion authority. One SHARED lease
/// covers both audits, excluding cooperating installers and collectors.
pub fn audit_media_for_registry_binding(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_registered: usize,
    max_scanned: usize,
) -> Result<MediaBindingPreflight, MediaGcPlanError> {
    if !(1..=10_000).contains(&max_registered) || !(1..=10_000).contains(&max_scanned) {
        return Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput));
    }
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaGcPlanError::Lease)?;
    audit_media_for_registry_binding_under_lease(store, registry, max_registered, max_scanned)
}

fn audit_media_for_registry_binding_under_lease(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    max_registered: usize,
    max_scanned: usize,
) -> Result<MediaBindingPreflight, MediaGcPlanError> {
    let registered = audit_registered_media_integrity_under_lease(store, registry, max_registered)?;
    let orphans = plan_unregistered_media_gc_under_lease(store, registry, max_scanned)?;
    Ok(MediaBindingPreflight {
        registered,
        orphans,
    })
}

/// Explicitly bind an existing registry only after an entire, bounded
/// registered-media integrity audit AND an orphan-free published-media scan.
/// A single EXCLUSIVE lease spans both preflights and the SQLite binding
/// commit, excluding cooperating installers, readers and garbage collectors
/// throughout. This is intentionally separate from destructive maintenance.
///
/// Reject oversized scans, corrupt registered bytes, missing notices and any
/// unexplained published object. The caller must additionally require an
/// existing registry outside the media root and explicit operator approval.
/// Non-cooperating direct SQLite writers and hostile directory races are
/// outside this advisory lease protocol; no publisher trust is conferred.
pub fn bind_registry_after_complete_media_audit(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    max_registered: usize,
    max_scanned: usize,
) -> Result<RegisteredMediaAudit, MediaGcPlanError> {
    if !(1..=10_000).contains(&max_registered) || !(1..=10_000).contains(&max_scanned) {
        return Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput));
    }
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_exclusive()
        .map_err(MediaGcPlanError::Lease)?;
    let preflight =
        audit_media_for_registry_binding_under_lease(store, registry, max_registered, max_scanned)?;
    if !preflight.orphans.candidates.is_empty() {
        return Err(MediaGcPlanError::UnregisteredOrphans {
            count: preflight.orphans.candidates.len(),
        });
    }
    registry
        .bind_media_root(store.root())
        .map_err(MediaGcPlanError::Registry)?;
    Ok(preflight.registered)
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
    fn bound_owner_release_rejects_wrong_registry_before_revocation() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"saved figure").unwrap();
        registry
            .register_verified_bytes(b"saved figure", &notice())
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::UserPin, "saved")
            .unwrap();
        assert!(matches!(
            release_bound_owner_and_reclaim_media(
                &store,
                &mut registry,
                OwnerKind::UserPin,
                "saved",
                10
            ),
            Err(MediaGcPlanError::Registry(MediaRegistryError::UnboundRoot))
        ));
        assert!(registry
            .owned_digest_size(OwnerKind::UserPin, "saved", &digest)
            .unwrap()
            .is_some());

        registry.bind_media_root(&root).unwrap();
        let other_root = root.with_extension("other");
        fs::create_dir(&other_root).unwrap();
        let other_store = MediaObjectStore::new(&other_root);
        assert!(matches!(
            release_bound_owner_and_reclaim_media(
                &other_store,
                &mut registry,
                OwnerKind::UserPin,
                "saved",
                10
            ),
            Err(MediaGcPlanError::Registry(MediaRegistryError::RootMismatch))
        ));
        assert!(registry
            .owned_digest_size(OwnerKind::UserPin, "saved", &digest)
            .unwrap()
            .is_some());
        assert_eq!(store.read_verified(&digest).unwrap(), b"saved figure");

        let outcome = release_bound_owner_and_reclaim_media(
            &store,
            &mut registry,
            OwnerKind::UserPin,
            "saved",
            10,
        )
        .unwrap();
        assert_eq!(outcome.released_objects, 1);
        assert_eq!(outcome.cleanup.unwrap().removed_objects, 1);
        assert!(registry.registered_digest_size(&digest).unwrap().is_none());
        fs::remove_dir_all(other_root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bound_maintenance_rechecks_root_identity_before_any_deletion() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"retired image").unwrap();
        registry
            .register_verified_bytes(b"retired image", &notice())
            .unwrap();
        // A caller bypassing the CLI must not reclaim bytes through the
        // production entry point with an unbound registry.
        assert!(matches!(
            run_bound_media_maintenance(&store, &mut registry, 10, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::UnboundRoot))
        ));
        assert_eq!(store.read_verified(&digest).unwrap(), b"retired image");
        assert_eq!(
            registry.registered_digest_size(&digest).unwrap(),
            Some(b"retired image".len() as u64)
        );

        registry.bind_media_root(&root).unwrap();
        let other_root = root.with_extension("different-root");
        fs::create_dir(&other_root).unwrap();
        let other_store = MediaObjectStore::new(&other_root);
        let other_digest = other_store.store_bytes(b"unregistered unknown").unwrap();
        assert!(matches!(
            run_bound_media_maintenance(&other_store, &mut registry, 10, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::RootMismatch))
        ));
        assert_eq!(
            other_store.read_verified(&other_digest).unwrap(),
            b"unregistered unknown"
        );
        assert_eq!(store.read_verified(&digest).unwrap(), b"retired image");

        let result = run_bound_media_maintenance(&store, &mut registry, 10, 10).unwrap();
        assert_eq!(result.registered.unwrap().removed_objects, 1);
        assert_eq!(result.orphans.unwrap().removed_objects, 0);
        assert!(registry.registered_digest_size(&digest).unwrap().is_none());
        assert_eq!(
            run_bound_media_maintenance(&store, &mut registry, 10, 10)
                .unwrap()
                .registered
                .unwrap()
                .removed_objects,
            0
        );
        fs::remove_dir_all(other_root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unbound_registry_audit_lists_verified_orphans_without_mutating_anything() {
        let (root, store, mut registry) = setup();
        let owned = store.store_bytes(b"retained map").unwrap();
        registry
            .register_verified_bytes(b"retained map", &notice())
            .unwrap();
        registry
            .add_owner(&owned, OwnerKind::Pack, "geography")
            .unwrap();
        let orphan = store.store_bytes(b"unknown atlas").unwrap();
        let preflight = audit_media_for_registry_binding(&store, &registry, 10, 10).unwrap();
        assert_eq!(preflight.registered.objects, 1);
        assert_eq!(
            preflight.registered.verified_bytes,
            b"retained map".len() as u64
        );
        assert_eq!(preflight.orphans.candidates.len(), 1);
        assert_eq!(preflight.orphans.candidates[0].digest, orphan);
        assert_eq!(
            preflight.orphans.reclaimable_bytes,
            b"unknown atlas".len() as u64
        );
        assert!(matches!(
            registry.require_media_root_binding(&root),
            Err(MediaRegistryError::UnboundRoot)
        ));
        assert!(matches!(
            audit_media_for_registry_binding(&store, &registry, 10, 0),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert_eq!(store.read_verified(&owned).unwrap(), b"retained map");
        assert_eq!(store.read_verified(&orphan).unwrap(), b"unknown atlas");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusive_registry_binding_requires_complete_clean_media_and_is_idempotent() {
        let (root, store, mut registry) = setup();
        let owned = store.store_bytes(b"owned figure").unwrap();
        registry
            .register_verified_bytes(b"owned figure", &notice())
            .unwrap();
        registry
            .add_owner(&owned, OwnerKind::UserPin, "saved")
            .unwrap();
        let orphan = store.store_bytes(b"unknown source").unwrap();
        assert!(matches!(
            bind_registry_after_complete_media_audit(&store, &mut registry, 10, 10),
            Err(MediaGcPlanError::UnregisteredOrphans { count: 1 })
        ));
        assert!(matches!(
            registry.require_media_root_binding(&root),
            Err(MediaRegistryError::UnboundRoot)
        ));
        assert_eq!(store.read_verified(&orphan).unwrap(), b"unknown source");
        assert!(matches!(
            bind_registry_after_complete_media_audit(&store, &mut registry, 0, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert!(matches!(
            registry.require_media_root_binding(&root),
            Err(MediaRegistryError::UnboundRoot)
        ));
        assert_eq!(
            collect_unregistered_media_orphans(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            1
        );
        let audit =
            bind_registry_after_complete_media_audit(&store, &mut registry, 10, 10).unwrap();
        assert_eq!(audit.objects, 1);
        assert_eq!(audit.verified_bytes, b"owned figure".len() as u64);
        registry.require_media_root_binding(&root).unwrap();
        assert_eq!(
            bind_registry_after_complete_media_audit(&store, &mut registry, 10, 10).unwrap(),
            audit
        );
        assert_eq!(store.read_verified(&owned).unwrap(), b"owned figure");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusive_registry_binding_waits_for_active_shared_media_reader() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (root, store, _) = setup();
        let db = root.with_extension("bind-reservation.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        let digest = store.store_bytes(b"locked visual").unwrap();
        registry
            .register_verified_bytes(b"locked visual", &notice())
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::UserPin, "reader")
            .unwrap();
        drop(registry);

        let reader_lease = MediaLeaseCoordinator::new(&root).acquire_shared().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let other_root = root.clone();
        let other_db = db.clone();
        let worker = std::thread::spawn(move || {
            let store = MediaObjectStore::new(&other_root);
            let mut registry = MediaRegistry::open_existing(&other_db).unwrap();
            ready_tx.send(()).unwrap();
            result_tx
                .send(bind_registry_after_complete_media_audit(
                    &store,
                    &mut registry,
                    10,
                    10,
                ))
                .unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            result_rx.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        let registry = MediaRegistry::open_existing(&db).unwrap();
        assert!(matches!(
            registry.require_media_root_binding(&root),
            Err(MediaRegistryError::UnboundRoot)
        ));
        drop(registry);
        drop(reader_lease);
        let audit = result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(audit.objects, 1);
        worker.join().unwrap();
        let registry = MediaRegistry::open_existing(&db).unwrap();
        registry.require_media_root_binding(&root).unwrap();
        assert_eq!(store.read_verified(&digest).unwrap(), b"locked visual");
        drop(registry);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(db).unwrap();
    }

    #[test]
    fn exclusive_registry_binding_rejects_corrupt_owned_bytes_without_binding() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"owned image").unwrap();
        registry
            .register_verified_bytes(b"owned image", &notice())
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Pack, "biology")
            .unwrap();
        fs::write(root.join(&digest[..2]).join(&digest), b"corrupted image").unwrap();
        assert!(matches!(
            bind_registry_after_complete_media_audit(&store, &mut registry, 10, 10),
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        assert!(matches!(
            registry.require_media_root_binding(&root),
            Err(MediaRegistryError::UnboundRoot)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn releasing_pack_reclaims_only_final_owner_and_preserves_pins_and_other_packs() {
        let (root, store, mut registry) = setup();
        let shared = store.store_bytes(b"shared figure").unwrap();
        registry
            .register_verified_bytes(b"shared figure", &notice())
            .unwrap();
        registry
            .add_owner(&shared, OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .add_owner(&shared, OwnerKind::Pack, "mathematics")
            .unwrap();
        registry
            .add_owner(&shared, OwnerKind::UserPin, "saved")
            .unwrap();
        let physics_only = store.store_bytes(b"private diagram").unwrap();
        registry
            .register_verified_bytes(b"private diagram", &notice())
            .unwrap();
        registry
            .add_owner(&physics_only, OwnerKind::Pack, "physics")
            .unwrap();
        let result =
            release_owner_and_reclaim_media(&store, &mut registry, OwnerKind::Pack, "physics", 10)
                .unwrap();
        assert_eq!(result.released_objects, 2);
        assert_eq!(result.cleanup.unwrap().removed_objects, 1);
        assert_eq!(store.read_verified(&shared).unwrap(), b"shared figure");
        assert!(registry
            .registered_digest_size(&physics_only)
            .unwrap()
            .is_none());
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "mathematics", &shared)
                .unwrap(),
            Some(b"shared figure".len() as u64)
        );
        let repeat =
            release_owner_and_reclaim_media(&store, &mut registry, OwnerKind::Pack, "physics", 10)
                .unwrap();
        assert_eq!(repeat.released_objects, 0);
        assert_eq!(repeat.cleanup.unwrap().removed_objects, 0);

        let other = release_owner_and_reclaim_media(
            &store,
            &mut registry,
            OwnerKind::Pack,
            "mathematics",
            10,
        )
        .unwrap();
        assert_eq!(other.released_objects, 1);
        assert_eq!(other.cleanup.unwrap().removed_objects, 0);
        assert_eq!(store.read_verified(&shared).unwrap(), b"shared figure");
        let pin =
            release_owner_and_reclaim_media(&store, &mut registry, OwnerKind::UserPin, "saved", 10)
                .unwrap();
        assert_eq!(pin.released_objects, 1);
        assert_eq!(pin.cleanup.unwrap().removed_objects, 1);
        assert!(registry.registered_digest_size(&shared).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn release_reports_committed_owner_removal_even_if_gc_fails() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"broken offline figure").unwrap();
        registry
            .register_verified_bytes(b"broken offline figure", &notice())
            .unwrap();
        registry
            .add_owner(&digest, OwnerKind::Cache, "offline")
            .unwrap();
        assert!(matches!(
            release_owner_and_reclaim_media(&store, &mut registry, OwnerKind::Cache, "offline", 0),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert!(registry
            .owned_digest_size(OwnerKind::Cache, "offline", &digest)
            .unwrap()
            .is_some());
        fs::write(
            root.join(&digest[..2]).join(&digest),
            b"tampered offline figure",
        )
        .unwrap();
        let outcome =
            release_owner_and_reclaim_media(&store, &mut registry, OwnerKind::Cache, "offline", 10)
                .unwrap();
        assert_eq!(outcome.released_objects, 1);
        assert!(matches!(
            outcome.cleanup,
            Err(MediaGcPlanError::Store(MediaObjectError::DigestMismatch))
        ));
        assert!(registry
            .owned_media(OwnerKind::Cache, "offline")
            .unwrap()
            .is_empty());
        assert_eq!(registry.unowned_digests().unwrap(), vec![digest.clone()]);
        // A later cleanup can be retried without rerunning owner release.
        fs::write(
            root.join(&digest[..2]).join(&digest),
            b"broken offline figure",
        )
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
    fn bounded_maintenance_reclaims_registered_and_orphaned_media_but_keeps_pins() {
        let (root, store, mut registry) = setup();
        let mut unowned = Vec::new();
        for content in [b"obsolete figure".as_slice(), b"obsolete map".as_slice()] {
            let digest = store.store_bytes(content).unwrap();
            registry
                .register_verified_bytes(content, &notice())
                .unwrap();
            unowned.push(digest);
        }
        let orphan = store.store_bytes(b"interrupted import").unwrap();
        let pinned = store.store_bytes(b"saved diagram").unwrap();
        registry
            .register_verified_bytes(b"saved diagram", &notice())
            .unwrap();
        registry
            .add_owner(&pinned, OwnerKind::UserPin, "saved")
            .unwrap();

        assert!(matches!(
            run_bounded_media_maintenance(&store, &mut registry, 10, 0),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert!(matches!(
            run_bounded_media_maintenance(&store, &mut registry, 0, 10),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert_eq!(registry.unowned_digests().unwrap().len(), 2);

        let result = run_bounded_media_maintenance(&store, &mut registry, 10, 10).unwrap();
        assert_eq!(result.registered.unwrap().removed_objects, 2);
        assert_eq!(result.orphans.unwrap().removed_objects, 1);
        for digest in unowned {
            assert!(registry.registered_digest_size(&digest).unwrap().is_none());
        }
        assert!(matches!(
            store.read_verified(&orphan),
            Err(MediaObjectError::Io(_))
        ));
        assert_eq!(store.read_verified(&pinned).unwrap(), b"saved diagram");
        let repeat = run_bounded_media_maintenance(&store, &mut registry, 10, 10).unwrap();
        assert_eq!(repeat.registered.unwrap().removed_objects, 0);
        assert_eq!(repeat.orphans.unwrap().removed_objects, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn maintenance_orphan_phase_recovers_registered_postcommit_unlink_failure() {
        let (root, store, mut registry) = setup();
        let digest = store.store_bytes(b"committed then interrupted").unwrap();
        registry
            .register_verified_bytes(b"committed then interrupted", &notice())
            .unwrap();
        let orphan = store.store_bytes(b"prior orphan").unwrap();
        let pinned = store.store_bytes(b"pinned media").unwrap();
        registry
            .register_verified_bytes(b"pinned media", &notice())
            .unwrap();
        registry
            .add_owner(&pinned, OwnerKind::UserPin, "saved")
            .unwrap();

        let outcome =
            run_bounded_media_maintenance_with_unlink(&store, &mut registry, 10, 10, |_| {
                Err(MediaObjectError::UnsafeObject)
            })
            .unwrap();
        assert!(matches!(
            outcome.registered,
            Err(MediaGcPlanError::PostCommitDeletion {
                completed_objects: 0,
                ..
            })
        ));
        let orphan_sweep = outcome.orphans.unwrap();
        assert_eq!(orphan_sweep.removed_objects, 2);
        assert_eq!(
            orphan_sweep.reclaimed_bytes,
            (b"committed then interrupted".len() + b"prior orphan".len()) as u64
        );
        assert!(registry.registered_digest_size(&digest).unwrap().is_none());
        assert!(matches!(
            store.read_verified(&orphan),
            Err(MediaObjectError::Io(_))
        ));
        assert_eq!(store.read_verified(&pinned).unwrap(), b"pinned media");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scheduled_bound_maintenance_respects_interval_and_retains_pinned_media() {
        let (root, store, mut registry) = setup();
        registry.bind_media_root(&root).unwrap();
        let pinned = store.store_bytes(b"saved diagram").unwrap();
        registry
            .register_verified_bytes(b"saved diagram", &notice())
            .unwrap();
        registry
            .add_owner(&pinned, OwnerKind::UserPin, "saved")
            .unwrap();
        let stale = store.store_bytes(b"obsolete diagram").unwrap();
        registry
            .register_verified_bytes(b"obsolete diagram", &notice())
            .unwrap();
        let first =
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1000).unwrap();
        match first {
            ScheduledMediaMaintenance::Ran {
                maintenance,
                checkpoint,
            } => {
                assert_eq!(maintenance.registered.unwrap().removed_objects, 1);
                assert_eq!(maintenance.orphans.unwrap().removed_objects, 0);
                assert!(checkpoint.unwrap());
            }
            _ => panic!("first invocation must run"),
        }
        assert!(registry.registered_digest_size(&stale).unwrap().is_none());
        let orphan = store.store_bytes(b"interrupted download").unwrap();
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1059).unwrap(),
            ScheduledMediaMaintenance::NotDue {
                next_due_epoch_seconds: 1060
            }
        ));
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 999).unwrap(),
            ScheduledMediaMaintenance::NotDue { .. }
        ));
        assert_eq!(store.read_verified(&orphan).unwrap(), b"interrupted download");
        let due =
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1060).unwrap();
        match due {
            ScheduledMediaMaintenance::Ran {
                maintenance,
                checkpoint,
            } => {
                assert_eq!(maintenance.registered.unwrap().removed_objects, 0);
                assert_eq!(maintenance.orphans.unwrap().removed_objects, 1);
                assert!(checkpoint.unwrap());
            }
            _ => panic!("due invocation must run"),
        }
        assert!(store.read_verified(&orphan).is_err());
        assert_eq!(store.read_verified(&pinned).unwrap(), b"saved diagram");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scheduled_maintenance_retries_failed_pass_without_advancing_checkpoint() {
        let (root, store, mut registry) = setup();
        registry.bind_media_root(&root).unwrap();
        let digest = store.store_bytes(b"damaged graph").unwrap();
        registry
            .register_verified_bytes(b"damaged graph", &notice())
            .unwrap();
        fs::write(root.join(&digest[..2]).join(&digest), b"corrupt graph").unwrap();
        let failure =
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1000).unwrap();
        match failure {
            ScheduledMediaMaintenance::Ran {
                maintenance,
                checkpoint,
            } => {
                assert!(maintenance.registered.is_err());
                assert!(!checkpoint.unwrap());
            }
            _ => panic!("first invocation must run"),
        }
        assert_eq!(registry.media_maintenance_last_success().unwrap(), None);
        fs::write(root.join(&digest[..2]).join(&digest), b"damaged graph").unwrap();
        let retried =
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1000).unwrap();
        assert!(matches!(
            retried,
            ScheduledMediaMaintenance::Ran {
                checkpoint: Ok(true),
                ..
            }
        ));
        assert_eq!(registry.media_maintenance_last_success().unwrap(), Some(1000));
        assert!(registry.registered_digest_size(&digest).unwrap().is_none());
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 0, 10, 60, 1060),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 1, 1060),
            Err(MediaGcPlanError::Registry(MediaRegistryError::InvalidInput))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scheduled_checkpoint_persists_across_registry_reopen_and_refuses_wrong_root() {
        let (root, store, _) = setup();
        let db = root.with_extension("scheduled.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        registry.bind_media_root(&root).unwrap();
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1000).unwrap(),
            ScheduledMediaMaintenance::Ran {
                checkpoint: Ok(true),
                ..
            }
        ));
        drop(registry);
        let mut registry = MediaRegistry::open_existing(&db).unwrap();
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1050).unwrap(),
            ScheduledMediaMaintenance::NotDue {
                next_due_epoch_seconds: 1060
            }
        ));
        let other_root = root.with_extension("wrong-root");
        fs::create_dir(&other_root).unwrap();
        let other = MediaObjectStore::new(&other_root);
        assert!(matches!(
            run_bound_media_maintenance_if_due(&other, &mut registry, 10, 10, 60, 1060),
            Err(MediaGcPlanError::Registry(MediaRegistryError::RootMismatch))
        ));
        assert_eq!(registry.media_maintenance_last_success().unwrap(), Some(1000));
        assert!(matches!(
            run_bound_media_maintenance_if_due(&store, &mut registry, 10, 10, 60, 1060).unwrap(),
            ScheduledMediaMaintenance::Ran {
                checkpoint: Ok(true),
                ..
            }
        ));
        drop(registry);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(other_root).unwrap();
        fs::remove_file(db).unwrap();
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

    /// Child-only test body: the parent invokes this exact test through the
    /// Rust test harness and expects an immediate exit after the first
    /// SQLite-committed, directory-synced file unlink.
    #[test]
    fn registered_gc_subprocess_killpoint_child() {
        let Some(root) = std::env::var_os("FW_STORE_002_GC_CHILD_ROOT") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let store = MediaObjectStore::new(&root);
        let database = root.with_extension("process-kill.sqlite");
        let mut registry = MediaRegistry::open(&database).unwrap();
        let _ = collect_unowned_registered_media_with_unlink(&store, &mut registry, 10, |digest| {
            let bytes = store.unlink_verified_unlocked(digest)?;
            // This is deliberately a process termination, not a returned
            // error: no Rust destructors or SQLite RAII cleanup run.
            if std::env::var_os("FW_STORE_002_GC_CHILD_ROOT").is_some() {
                std::process::exit(77);
            }
            Ok(bytes)
        });
        panic!("child must terminate at the post-commit unlink killpoint");
    }

    #[test]
    fn process_exit_after_first_unlink_recovers_orphans_on_reopen() {
        let (root, store, _) = setup();
        let database = root.with_extension("process-kill.sqlite");
        let mut registry = MediaRegistry::open(&database).unwrap();
        let mut unowned = Vec::new();
        for content in [
            b"first chart".as_slice(),
            b"second map".as_slice(),
            b"third image".as_slice(),
        ] {
            let digest = store.store_bytes(content).unwrap();
            registry
                .register_verified_bytes(content, &notice())
                .unwrap();
            unowned.push(digest);
        }
        let pinned = store.store_bytes(b"saved figure").unwrap();
        registry
            .register_verified_bytes(b"saved figure", &notice())
            .unwrap();
        registry
            .add_owner(&pinned, OwnerKind::UserPin, "saved")
            .unwrap();
        drop(registry);

        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("media_gc::tests::registered_gc_subprocess_killpoint_child")
            .env("FW_STORE_002_GC_CHILD_ROOT", &root)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(77),
            "child did not reach killpoint: {}",
            String::from_utf8_lossy(&child.stderr)
        );

        // The OS must release the dead child's EXCLUSIVE SQLite lease.
        // The committed registry no longer references any of the three
        // unowned objects, although only one file was actually removed.
        let mut registry = MediaRegistry::open(&database).unwrap();
        for digest in &unowned {
            assert!(registry.registered_digest_size(digest).unwrap().is_none());
        }
        assert_eq!(
            registry.registered_digest_size(&pinned).unwrap(),
            Some(b"saved figure".len() as u64)
        );
        let orphans = plan_unregistered_media_gc(&store, &registry, 10).unwrap();
        assert_eq!(orphans.candidates.len(), 2);
        let recovered = collect_unregistered_media_orphans(&store, &mut registry, 10).unwrap();
        assert_eq!(recovered.removed_objects, 2);
        assert_eq!(recovered.reclaimed_bytes, orphans.reclaimable_bytes);
        assert_eq!(store.read_verified(&pinned).unwrap(), b"saved figure");
        assert_eq!(
            collect_unregistered_media_orphans(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            0
        );
        drop(registry);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(database).unwrap();
    }

    #[test]
    fn partial_unlink_failure_recovers_remaining_orphans_after_registry_reopen() {
        let (root, store, _) = setup();
        // Persist the registry OUTSIDE the published-digest root: the scanner
        // deliberately refuses unrelated SQLite files inside the media root.
        let db = root.with_extension("registry.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        let mut digests = Vec::new();
        for content in [
            b"alpha graph".as_slice(),
            b"beta graph".as_slice(),
            b"gamma graph".as_slice(),
        ] {
            let digest = store.store_bytes(content).unwrap();
            registry
                .register_verified_bytes(content, &notice())
                .unwrap();
            digests.push(digest);
        }
        let pin = store.store_bytes(b"saved thumbnail").unwrap();
        registry
            .register_verified_bytes(b"saved thumbnail", &notice())
            .unwrap();
        registry
            .add_owner(&pin, OwnerKind::UserPin, "saved")
            .unwrap();

        let mut deleted = 0usize;
        let failed =
            collect_unowned_registered_media_with_unlink(&store, &mut registry, 10, |digest| {
                if deleted == 1 {
                    return Err(MediaObjectError::UnsafeObject);
                }
                deleted += 1;
                store.unlink_verified_unlocked(digest)
            });
        assert!(matches!(
            failed,
            Err(MediaGcPlanError::PostCommitDeletion {
                completed_objects: 1,
                ..
            })
        ));
        // The SQL transaction committed the three unowned object removals
        // before touching any file. One file is now gone, two survive.
        for digest in &digests {
            assert!(registry.registered_digest_size(digest).unwrap().is_none());
        }
        assert_eq!(registry.registered_digest_size(&pin).unwrap(), Some(15));
        let published = store.published_object_digests(10).unwrap();
        assert_eq!(published.len(), 3);
        let remaining_bytes: u64 = published
            .iter()
            .filter(|digest| *digest != &pin)
            .map(|digest| {
                fs::metadata(root.join(&digest[..2]).join(digest))
                    .unwrap()
                    .len()
            })
            .sum();
        drop(registry);

        // Restart/reopen after the injected crash window. The separate
        // orphan sweep must reclaim only the two files remaining unregistered.
        let mut registry = MediaRegistry::open(&db).unwrap();
        let recovered = collect_unregistered_media_orphans(&store, &mut registry, 10).unwrap();
        assert_eq!(recovered.removed_objects, 2);
        assert_eq!(recovered.reclaimed_bytes, remaining_bytes);
        assert_eq!(
            store.published_object_digests(10).unwrap(),
            vec![pin.clone()]
        );
        assert_eq!(store.read_verified(&pin).unwrap(), b"saved thumbnail");
        assert_eq!(
            collect_unregistered_media_orphans(&store, &mut registry, 10)
                .unwrap()
                .removed_objects,
            0
        );
        drop(registry);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(db).unwrap();
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
    fn registered_integrity_audit_waits_for_exclusive_gc_lease_across_inventory() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (root, store, _) = setup();
        let database = root.with_extension("registered-audit.sqlite");
        let mut registry = MediaRegistry::open(&database).unwrap();
        for content in [b"first chart".as_slice(), b"second chart".as_slice()] {
            let digest = store.store_bytes(content).unwrap();
            registry
                .register_verified_bytes(content, &notice())
                .unwrap();
            registry
                .add_owner(&digest, OwnerKind::Pack, "physics")
                .unwrap();
        }
        drop(registry);

        let exclusive = MediaLeaseCoordinator::new(&root)
            .acquire_exclusive()
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker_root = root.clone();
        let worker_database = database.clone();
        let worker = std::thread::spawn(move || {
            let store = MediaObjectStore::new(&worker_root);
            let registry = MediaRegistry::open(&worker_database).unwrap();
            started_tx.send(()).unwrap();
            let audit = audit_registered_media_integrity(&store, &registry, 10).unwrap();
            finished_tx.send(audit).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            finished_rx.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(exclusive);
        let audit = finished_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(audit.objects, 2);
        assert_eq!(
            audit.verified_bytes,
            (b"first chart".len() + b"second chart".len()) as u64
        );
        worker.join().unwrap();
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(database).unwrap();
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
    fn both_advisory_gc_planners_wait_for_exclusive_media_lease() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (root, store, _) = setup();
        let database = root.with_extension("advisory-gc.sqlite");
        let mut registry = MediaRegistry::open(&database).unwrap();
        let registered = store.store_bytes(b"registered chart").unwrap();
        registry
            .register_verified_bytes(b"registered chart", &notice())
            .unwrap();
        let orphan = store.store_bytes(b"unregistered diagram").unwrap();
        drop(registry);

        let exclusive = MediaLeaseCoordinator::new(&root)
            .acquire_exclusive()
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let mut workers = Vec::new();
        for kind in ["registered", "orphan"] {
            let worker_root = root.clone();
            let worker_database = database.clone();
            let started_tx = started_tx.clone();
            let done_tx = done_tx.clone();
            workers.push(std::thread::spawn(move || {
                let store = MediaObjectStore::new(&worker_root);
                let registry = MediaRegistry::open(&worker_database).unwrap();
                started_tx.send(()).unwrap();
                let plan = if kind == "registered" {
                    plan_unowned_media_gc(&store, &registry, 10)
                } else {
                    plan_unregistered_media_gc(&store, &registry, 10)
                }
                .unwrap();
                done_tx.send((kind, plan)).unwrap();
            }));
        }
        for _ in 0..2 {
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(exclusive);
        let mut results = std::collections::BTreeMap::new();
        for _ in 0..2 {
            let (kind, plan) = done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            results.insert(kind, plan);
        }
        assert_eq!(results["registered"].candidates[0].digest, registered);
        assert_eq!(results["registered"].candidates.len(), 1);
        assert_eq!(results["orphan"].candidates[0].digest, orphan);
        assert_eq!(results["orphan"].candidates.len(), 1);
        for worker in workers {
            worker.join().unwrap();
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(database).unwrap();
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
