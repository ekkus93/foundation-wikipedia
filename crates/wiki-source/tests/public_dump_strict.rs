use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_source::public_dump_pinned::{
    parse_pinned_inventory, stage_pinned_member, PinnedInventory,
};
use wiki_source::public_dump_stage::StagedDumpError;
use wiki_source::public_dump_strict::verify_pinned_staged_export_exact;

fn fixture() -> (PathBuf, PinnedInventory) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-exact-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let paths: [(&str, &[u8]); 2] = [
        ("enwiki-2026-10-01-p1p100.xml.bz2", b"first"),
        ("nested/enwiki-2026-10-01-p101p200.xml.bz2", b"second"),
    ];
    let mut raw = String::new();
    for (name, data) in paths {
        use std::fmt::Write as _;
        writeln!(raw, "{:x}  {name}", Sha256::digest(data)).unwrap();
    }
    let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let inventory =
        parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
    for (index, member) in inventory.members().iter().enumerate() {
        let data = paths
            .iter()
            .find(|(name, _)| *name == member.relative_path)
            .unwrap()
            .1;
        stage_pinned_member(&root, &inventory, index, &mut Cursor::new(data), 1024).unwrap();
    }
    (root, inventory)
}

#[test]
fn exact_tree_accepts_only_declared_verified_files() {
    let (root, inventory) = fixture();
    assert_eq!(
        verify_pinned_staged_export_exact(&root, &inventory, 1024).unwrap(),
        11
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn exact_tree_rejects_unlisted_files_and_directories() {
    let (root, inventory) = fixture();
    let extra = root.join("unlisted.xml.bz2");
    fs::write(&extra, b"extra").unwrap();
    assert!(matches!(
        verify_pinned_staged_export_exact(&root, &inventory, 1024),
        Err(StagedDumpError::InvalidInventory)
    ));
    fs::remove_file(extra).unwrap();

    let extra = root.join("other");
    fs::create_dir(&extra).unwrap();
    assert!(matches!(
        verify_pinned_staged_export_exact(&root, &inventory, 1024),
        Err(StagedDumpError::InvalidInventory)
    ));
    fs::remove_dir(extra).unwrap();

    let extra = root.join("nested/.incomplete.tmp");
    fs::write(&extra, b"partial").unwrap();
    assert!(matches!(
        verify_pinned_staged_export_exact(&root, &inventory, 1024),
        Err(StagedDumpError::InvalidInventory)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn exact_tree_rejects_symlink_aliases() {
    use std::os::unix::fs::symlink;
    let (root, inventory) = fixture();
    symlink(
        root.join("enwiki-2026-10-01-p1p100.xml.bz2"),
        root.join("alias"),
    )
    .unwrap();
    assert!(matches!(
        verify_pinned_staged_export_exact(&root, &inventory, 1024),
        Err(StagedDumpError::UnsafeStagingPath)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn exact_tree_still_rejects_tampered_declared_bytes() {
    let (root, inventory) = fixture();
    fs::write(root.join("enwiki-2026-10-01-p1p100.xml.bz2"), b"bad").unwrap();
    assert!(matches!(
        verify_pinned_staged_export_exact(&root, &inventory, 1024),
        Err(StagedDumpError::Member(_))
    ));
    fs::remove_dir_all(root).unwrap();
}
