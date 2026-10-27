//! Black-box acceptance of the public media-maintenance command dispatcher.
//! The internal maintenance unit tests alone cannot catch CLI flag rejection.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::{MediaNotice, MediaRegistry, OwnerKind};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn fixture() -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "wiki-media-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let root = base.join("media");
    fs::create_dir_all(&root).unwrap();
    (base.clone(), root, base.join("owners.sqlite"))
}

fn invoke(mode: &str, root: &Path, db: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args([
            "media-maintenance",
            mode,
            root.to_str().unwrap(),
            db.to_str().unwrap(),
            "10",
            "10",
        ])
        .output()
        .unwrap()
}

fn notice() -> MediaNotice {
    MediaNotice {
        mime: "image/png".into(),
        source_url: "https://commons.wikimedia.org/wiki/File:Map.png".into(),
        creator: "Example creator".into(),
        license: "CC BY-SA 4.0".into(),
    }
}

#[test]
fn public_cli_exposes_unbound_audit_and_explicit_binding_without_implicit_deletion() {
    let (base, root, db) = fixture();
    let store = MediaObjectStore::new(&root);
    let mut registry = MediaRegistry::open(&db).unwrap();
    let pinned = store.store_bytes(b"pinned map").unwrap();
    registry
        .register_verified_bytes(b"pinned map", &notice())
        .unwrap();
    registry
        .add_owner(&pinned, OwnerKind::UserPin, "reader")
        .unwrap();
    let stale = store.store_bytes(b"stale map").unwrap();
    registry
        .register_verified_bytes(b"stale map", &notice())
        .unwrap();
    drop(registry);

    let unbound = invoke("--audit-unbound", &root, &db);
    assert!(
        unbound.status.success(),
        "{}",
        String::from_utf8_lossy(&unbound.stderr)
    );
    assert!(String::from_utf8_lossy(&unbound.stdout).contains("READ ONLY:"));
    assert!(!invoke("--dry-run", &root, &db).status.success());
    assert!(!invoke("--confirm-delete", &root, &db).status.success());
    assert_eq!(store.read_verified(&stale).unwrap(), b"stale map");

    let bound = invoke("--bind-registry", &root, &db);
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    assert!(String::from_utf8_lossy(&bound.stdout).contains("no files deleted"));
    assert!(invoke("--bind-registry", &root, &db).status.success());
    assert!(invoke("--dry-run", &root, &db).status.success());
    assert_eq!(store.read_verified(&stale).unwrap(), b"stale map");

    let deleted = invoke("--confirm-delete", &root, &db);
    assert!(
        deleted.status.success(),
        "{}",
        String::from_utf8_lossy(&deleted.stderr)
    );
    assert!(String::from_utf8_lossy(&deleted.stdout).contains("Reclaimed 1 registered objects"));
    assert_eq!(store.read_verified(&pinned).unwrap(), b"pinned map");
    assert!(store.read_verified(&stale).is_err());
    assert!(!invoke("--wrong-mode", &root, &db).status.success());
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn public_cli_audits_orphans_but_refuses_binding_or_deletion() {
    let (base, root, db) = fixture();
    let registry = MediaRegistry::open(&db).unwrap();
    drop(registry);
    let store = MediaObjectStore::new(&root);
    let orphan = store.store_bytes(b"unregistered image").unwrap();
    let audit = invoke("--audit-unbound", &root, &db);
    assert!(
        audit.status.success(),
        "{}",
        String::from_utf8_lossy(&audit.stderr)
    );
    assert!(String::from_utf8_lossy(&audit.stdout).contains(&orphan));
    assert!(!invoke("--bind-registry", &root, &db).status.success());
    assert!(!invoke("--confirm-delete", &root, &db).status.success());
    assert_eq!(store.read_verified(&orphan).unwrap(), b"unregistered image");
    fs::remove_dir_all(base).unwrap();
}
