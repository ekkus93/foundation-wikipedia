//! Explicit local maintenance; never creates an ownership database.

use std::fs;
use std::path::{Component, Path};

use wiki_store::media_gc::{
    bind_registry_after_complete_media_audit, plan_unowned_media_gc, plan_unregistered_media_gc,
    run_bounded_media_maintenance,
};
use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

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
    if !matches!(mode, "--dry-run" | "--confirm-delete" | "--bind-registry") {
        return Err("explicit --bind-registry, --dry-run or --confirm-delete required".into());
    }
    if !(1..=10_000).contains(&max_registered) || !(1..=10_000).contains(&max_scanned) {
        return Err("media budgets must each be between 1 and 10000".into());
    }
    safe_directory(root)?;
    let root_canonical = fs::canonicalize(root).map_err(|error| format!("media root: {error}"))?;
    let registry_canonical = fs::canonicalize(registry_path)
        .map_err(|error| format!("existing registry required: {error}"))?;
    if registry_canonical.starts_with(&root_canonical) {
        return Err("registry must be outside the media-object root".into());
    }
    let mut registry = MediaRegistry::open_existing(registry_path)
        .map_err(|error| format!("cannot open existing media registry: {error:?}"))?;
    let store = MediaObjectStore::new(root);
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
    let outcome = run_bounded_media_maintenance(&store, &mut registry, max_registered, max_scanned)
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
        assert!(run("--bind-registry", &root, &db, 10, 10).is_err());
        assert!(run("--confirm-delete", &root, &db, 10, 10).is_err());
        assert_eq!(
            store.read_verified(&orphan).unwrap(),
            b"unregistered payload"
        );
        fs::remove_dir_all(base).unwrap();
    }
}
