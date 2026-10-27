//! Explicit local maintenance; never creates an ownership database.

use std::fs;
use std::path::{Component, Path};

use wiki_store::media_gc::{
    audit_media_for_registry_binding, bind_registry_after_complete_media_audit,
    plan_unowned_media_gc, plan_unregistered_media_gc, preview_bound_owner_release,
    release_bound_owner_and_reclaim_media, run_bound_media_maintenance,
};
use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::{MediaRegistry, OwnerKind};

fn safe_directory(path: &Path) -> Result<(), String> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("media root cannot contain parent traversal".into());
    }
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| format!("cannot inspect media root: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("media root cannot contain symlinks or non-directories".into());
        }
    }
    Ok(())
}

/// A missing/wrong registry could make owned media look like unregistered
/// orphans. Require an existing schema outside the media object tree and an
/// explicit destructive confirmation; --dry-run is always read-only.
pub fn run(
    mode: &str,
    root: &Path,
    registry_path: &Path,
    max_registered: usize,
    max_scanned: usize,
) -> Result<(), String> {
    if !matches!(
        mode,
        "--dry-run" | "--confirm-delete" | "--bind-registry" | "--audit-unbound"
    ) {
        return Err(
            "explicit --audit-unbound, --bind-registry, --dry-run or --confirm-delete required"
                .into(),
        );
    }
    if !(1..=10_000).contains(&max_registered) || !(1..=10_000).contains(&max_scanned) {
        return Err("media budgets must each be between 1 and 10000".into());
    }
    safe_directory(root)?;
    #[cfg(unix)]
    if matches!(mode, "--confirm-delete" | "--bind-registry") {
        use std::os::unix::fs::PermissionsExt;
        // The SQLite-backed lease only coordinates cooperating processes.
        // Never authorize deletion or binding in a root writable by another
        // local user or group, even if all digest files look valid right now.
        let permissions = fs::symlink_metadata(root)
            .map_err(|error| format!("cannot inspect media root permissions: {error}"))?
            .permissions();
        if permissions.mode() & 0o022 != 0 {
            return Err(
                "media root must not be group/world writable for destructive operations".into(),
            );
        }
    }
    let root_canonical = fs::canonicalize(root).map_err(|error| format!("media root: {error}"))?;
    let registry_canonical = fs::canonicalize(registry_path)
        .map_err(|error| format!("existing registry required: {error}"))?;
    if registry_canonical.starts_with(&root_canonical) {
        return Err("registry must be outside the media-object root".into());
    }
    let mut registry = MediaRegistry::open_existing(registry_path)
        .map_err(|error| format!("cannot open existing media registry: {error:?}"))?;
    let store = MediaObjectStore::new(root);
    if mode == "--audit-unbound" {
        // An unbound registry cannot use --dry-run (which requires a trusted
        // root binding). This explicit diagnostic has NO deletion authority.
        let audit =
            audit_media_for_registry_binding(&store, &registry, max_registered, max_scanned)
                .map_err(|error| format!("unbound media audit failed: {error:?}"))?;
        println!(
            "READ ONLY: {} registered objects ({} verified bytes); {} unregistered published objects ({} bytes)",
            audit.registered.objects,
            audit.registered.verified_bytes,
            audit.orphans.candidates.len(),
            audit.orphans.reclaimable_bytes
        );
        for orphan in &audit.orphans.candidates {
            println!(
                "Unregistered media: {} ({} bytes)",
                orphan.digest, orphan.bytes
            );
        }
        println!("No files deleted; no registry rows or root binding changed.");
        println!("Reconcile orphan provenance before binding; do not assume they are disposable.");
        return Ok(());
    }
    if mode == "--bind-registry" {
        // A wrong but initialized registry could otherwise be bound to a
        // populated media tree. One EXCLUSIVE lease must span both complete
        // audits and the binding commit, excluding cooperating installers/GC.
        let audit = bind_registry_after_complete_media_audit(
            &store,
            &mut registry,
            max_registered,
            max_scanned,
        )
        .map_err(|error| format!("registry integrity/binding rejected: {error:?}"))?;
        println!(
            "Registry bound to media root after auditing {} objects ({} bytes); no files deleted",
            audit.objects, audit.verified_bytes
        );
        return Ok(());
    }
    registry
        .require_media_root_binding(root)
        .map_err(|error| format!("media registry root binding required: {error:?}"))?;
    if mode == "--dry-run" {
        let registered = plan_unowned_media_gc(&store, &registry, max_registered)
            .map_err(|error| format!("registered preflight: {error:?}"))?;
        let orphans = plan_unregistered_media_gc(&store, &registry, max_scanned)
            .map_err(|error| format!("orphan preflight: {error:?}"))?;
        println!(
            "DRY RUN: {} unowned registered objects ({} bytes), {} unregistered orphans ({} bytes)",
            registered.candidates.len(),
            registered.reclaimable_bytes,
            orphans.candidates.len(),
            orphans.reclaimable_bytes
        );
        println!("No registry rows or media bytes were deleted.");
        return Ok(());
    }
    let outcome = run_bound_media_maintenance(&store, &mut registry, max_registered, max_scanned)
        .map_err(|error| format!("maintenance could not acquire lease: {error:?}"))?;
    match (outcome.registered, outcome.orphans) {
        (Ok(registered), Ok(orphans)) => {
            println!(
                "Reclaimed {} registered objects ({} bytes), {} orphans ({} bytes)",
                registered.removed_objects,
                registered.reclaimed_bytes,
                orphans.removed_objects,
                orphans.reclaimed_bytes
            );
            Ok(())
        }
        (registered, orphans) => Err(format!(
            "maintenance incomplete; registered phase: {registered:?}; orphan phase: {orphans:?}"
        )),
    }
}

/// Explicit owner removal for an operator-managed pack, cache or user pin.
/// This is NOT a pack uninstall transaction: the caller must separately
/// coordinate its installed-pack/catalog state. Never infer ownership from
/// a filename or an untrusted manifest; the operator supplies an exact ID.
pub fn release_owner(
    mode: &str,
    root: &Path,
    registry_path: &Path,
    kind: &str,
    owner_id: &str,
    max_candidates: usize,
) -> Result<(), String> {
    if !matches!(mode, "--dry-run" | "--confirm-release") {
        return Err("owner release requires --dry-run or --confirm-release".into());
    }
    if !(1..=10_000).contains(&max_candidates) {
        return Err("owner-release candidate limit must be between 1 and 10000".into());
    }
    let owner_kind = match kind {
        "pack" => OwnerKind::Pack,
        "cache" => OwnerKind::Cache,
        "pin" => OwnerKind::UserPin,
        _ => return Err("owner kind must be exactly pack, cache or pin".into()),
    };
    safe_directory(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::symlink_metadata(root)
            .map_err(|error| format!("cannot inspect media root permissions: {error}"))?
            .permissions();
        if permissions.mode() & 0o022 != 0 {
            return Err("owner release requires a media root not writable by group/others".into());
        }
    }
    let root_canonical = fs::canonicalize(root).map_err(|error| format!("media root: {error}"))?;
    let registry_canonical = fs::canonicalize(registry_path)
        .map_err(|error| format!("existing registry required: {error}"))?;
    if registry_canonical.starts_with(&root_canonical) {
        return Err("registry must be outside the media-object root".into());
    }
    let mut registry = MediaRegistry::open_existing(registry_path)
        .map_err(|error| format!("cannot open existing media registry: {error:?}"))?;
    let store = MediaObjectStore::new(root);
    if mode == "--dry-run" {
        let preview =
            preview_bound_owner_release(&store, &registry, owner_kind, owner_id, max_candidates)
                .map_err(|error| format!("owner-release preview rejected: {error:?}"))?;
        println!(
            "DRY RUN: {} claims for {kind}/{owner_id}; {} would become unowned ({} verified bytes); {} already unowned ({} verified bytes)",
            preview.owned_claims,
            preview.newly_unowned.candidates.len(),
            preview.newly_unowned.reclaimable_bytes,
            preview.previously_unowned.candidates.len(),
            preview.previously_unowned.reclaimable_bytes
        );
        if preview.exceeds_gc_budget {
            println!(
                "WARNING: predicted unowned candidates exceed the {}-object GC budget; confirmed release can commit while cleanup fails",
                max_candidates
            );
        }
        println!("ADVISORY ONLY: ownership can change before confirmation; no owner claims or media objects deleted.");
        return Ok(());
    }
    let outcome = release_bound_owner_and_reclaim_media(
        &store,
        &mut registry,
        owner_kind,
        owner_id,
        max_candidates,
    )
    .map_err(|error| format!("owner release rejected; no owner claims removed: {error:?}"))?;
    match outcome.cleanup {
        Ok(sweep) => {
            println!(
                "OWNER RELEASE COMMITTED: {} claims removed for {kind}/{owner_id}; reclaimed {} objects ({} bytes)",
                outcome.released_objects, sweep.removed_objects, sweep.reclaimed_bytes
            );
            Ok(())
        }
        Err(error) => Err(format!(
            "OWNER RELEASE COMMITTED: {} claims removed for {kind}/{owner_id}; cleanup incomplete: {error:?}. Do NOT retry the uninstall as a transaction; run bound media maintenance to retry cleanup.",
            outcome.released_objects
        )),
    }
}

/// One opt-in invocation from cron/systemd or a host application job.
/// Never starts a background daemon or silently enables destructive cleanup.
pub fn run_if_due(
    root: &Path,
    registry_path: &Path,
    max_registered: usize,
    max_scanned: usize,
    interval_seconds: u64,
) -> Result<(), String> {
    if !(1..=10_000).contains(&max_registered)
        || !(1..=10_000).contains(&max_scanned)
        || !(60..=2_592_000).contains(&interval_seconds)
    {
        return Err("scheduled budgets must be 1..10000 and interval 60..2592000 seconds".into());
    }
    safe_directory(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::symlink_metadata(root)
            .map_err(|error| format!("cannot inspect media root permissions: {error}"))?
            .permissions();
        if permissions.mode() & 0o022 != 0 {
            return Err("scheduled media root must not be group/world writable".into());
        }
    }
    let root_canonical = fs::canonicalize(root).map_err(|error| format!("media root: {error}"))?;
    let registry_canonical = fs::canonicalize(registry_path)
        .map_err(|error| format!("existing registry required: {error}"))?;
    if registry_canonical.starts_with(&root_canonical) {
        return Err("registry must be outside the media-object root".into());
    }
    let mut registry = MediaRegistry::open_existing(registry_path)
        .map_err(|error| format!("cannot open existing media registry: {error:?}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("invalid system clock: {error}"))?
        .as_secs();
    let store = MediaObjectStore::new(root);
    match wiki_store::media_gc::run_bound_media_maintenance_if_due(
        &store,
        &mut registry,
        max_registered,
        max_scanned,
        interval_seconds,
        now,
    )
    .map_err(|error| format!("scheduled maintenance rejected: {error:?}"))?
    {
        wiki_store::media_gc::ScheduledMediaMaintenance::NotDue {
            next_due_epoch_seconds,
        } => {
            println!("NOT DUE: next scheduled media cleanup at Unix time {next_due_epoch_seconds}; no files deleted");
            Ok(())
        }
        wiki_store::media_gc::ScheduledMediaMaintenance::Ran {
            maintenance,
            checkpoint,
        } => match (maintenance.registered, maintenance.orphans, checkpoint) {
            (Ok(registered), Ok(orphans), Ok(true)) => {
                println!(
                    "SCHEDULED RUN: reclaimed {} registered objects ({} bytes), {} orphans ({} bytes); success checkpoint persisted",
                    registered.removed_objects,
                    registered.reclaimed_bytes,
                    orphans.removed_objects,
                    orphans.reclaimed_bytes
                );
                Ok(())
            }
            (registered, orphans, checkpoint) => Err(format!(
                "scheduled maintenance incomplete or checkpoint failed; registered: {registered:?}; orphans: {orphans:?}; checkpoint: {checkpoint:?}"
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_store::media_ownership::{MediaNotice, OwnerKind};

    #[test]
    fn guarded_cli_dry_run_and_confirmed_cleanup_preserve_user_pin() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "wiki-pack-maintenance-{}-{stamp}",
            std::process::id()
        ));
        let root = base.join("media");
        fs::create_dir_all(&root).unwrap();
        let db = base.join("owners.sqlite");
        let store = MediaObjectStore::new(&root);
        assert!(run("--confirm-delete", &root, &db, 10, 10).is_err());
        assert!(!db.exists());
        let mut registry = MediaRegistry::open(&db).unwrap();
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Chart.png".into(),
            creator: "Example author".into(),
            license: "CC BY-SA 4.0".into(),
        };
        let pinned = store.store_bytes(b"saved graph").unwrap();
        registry
            .register_verified_bytes(b"saved graph", &notice)
            .unwrap();
        registry
            .add_owner(&pinned, OwnerKind::UserPin, "saved")
            .unwrap();
        let unowned = store.store_bytes(b"obsolete graph").unwrap();
        registry
            .register_verified_bytes(b"obsolete graph", &notice)
            .unwrap();
        drop(registry);
        assert!(run("--confirm-delete", &root, &db, 10, 10).is_err());
        run("--bind-registry", &root, &db, 10, 10).unwrap();
        run("--bind-registry", &root, &db, 10, 10).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
            assert!(run("--confirm-delete", &root, &db, 10, 10).is_err());
            assert!(run("--bind-registry", &root, &db, 10, 10).is_err());
            assert_eq!(store.read_verified(&pinned).unwrap(), b"saved graph");
            assert_eq!(store.read_verified(&unowned).unwrap(), b"obsolete graph");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let orphan = store.store_bytes(b"interrupted import").unwrap();
        let other_root = base.join("wrong-media");
        fs::create_dir_all(&other_root).unwrap();
        assert!(run("--confirm-delete", &other_root, &db, 10, 10).is_err());
        assert!(run("--bind-registry", &other_root, &db, 10, 10).is_err());

        assert!(run("no-confirm", &root, &db, 10, 10).is_err());
        assert!(run("--confirm-delete", &root, &db, 0, 10).is_err());
        assert!(run(
            "--confirm-delete",
            &root,
            &root.join("owners.sqlite"),
            10,
            10
        )
        .is_err());
        run("--dry-run", &root, &db, 10, 10).unwrap();
        assert!(store.read_verified(&orphan).is_ok());
        assert!(store.read_verified(&unowned).is_ok());

        run("--confirm-delete", &root, &db, 10, 10).unwrap();
        assert!(store.read_verified(&pinned).is_ok());
        assert!(store.read_verified(&orphan).is_err());
        assert!(store.read_verified(&unowned).is_err());
        let registry = MediaRegistry::open_existing(&db).unwrap();
        assert_eq!(registry.registered_digest_size(&pinned).unwrap(), Some(11));
        assert_eq!(registry.registered_digest_size(&unowned).unwrap(), None);
        drop(registry);
        run("--confirm-delete", &root, &db, 10, 10).unwrap();
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn registry_binding_refuses_unregistered_orphans_and_preserves_files() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "wiki-pack-bind-guard-{}-{stamp}",
            std::process::id()
        ));
        let root = base.join("media");
        fs::create_dir_all(&root).unwrap();
        let db = base.join("owners.sqlite");
        let registry = MediaRegistry::open(&db).unwrap();
        drop(registry);
        let store = MediaObjectStore::new(&root);
        let orphan = store.store_bytes(b"unregistered payload").unwrap();
        run("--audit-unbound", &root, &db, 10, 10).unwrap();
        assert!(run("--dry-run", &root, &db, 10, 10).is_err());
        assert!(run("--bind-registry", &root, &db, 10, 10).is_err());
        assert!(run("--confirm-delete", &root, &db, 10, 10).is_err());
        assert_eq!(
            store.read_verified(&orphan).unwrap(),
            b"unregistered payload"
        );
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn packaged_user_timer_is_opt_in_and_invokes_the_bounded_due_gate() {
        let service = include_str!("../systemd/foundation-wikipedia-media-maintenance.service");
        let timer = include_str!("../systemd/foundation-wikipedia-media-maintenance.timer");
        let environment = include_str!("../systemd/media-maintenance.env.example");

        // Never interpret user-controlled paths through a shell. Each
        // environment expansion is a single systemd ExecStart argument.
        assert!(service.contains("Type=oneshot"));
        assert!(service.contains("UMask=0077"));
        assert!(service
            .contains("EnvironmentFile=%h/.config/foundation-wikipedia/media-maintenance.env"));
        assert!(service.contains(
            "ExecStart=%h/.local/bin/wiki-pack media-maintenance --run-if-due ${MEDIA_ROOT} ${REGISTRY_PATH} ${MAX_REGISTERED} ${MAX_SCANNED} ${INTERVAL_SECONDS}"
        ));
        assert!(!service.contains("/bin/sh"));
        assert!(!service.contains("ExecStartPre="));
        assert!(timer.contains("OnCalendar=hourly"));
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("Unit=foundation-wikipedia-media-maintenance.service"));
        assert!(!timer.contains("[Install]\nWantedBy=default.target"));
        for variable in [
            "MEDIA_ROOT=",
            "REGISTRY_PATH=",
            "MAX_REGISTERED=",
            "MAX_SCANNED=",
            "INTERVAL_SECONDS=",
        ] {
            assert!(environment.contains(variable));
        }
     }
}
