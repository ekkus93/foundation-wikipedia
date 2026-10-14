//! Regression checks for untrusted group/world-writable dump staging roots.
#[cfg(unix)]
#[test]
fn writable_dump_staging_root_is_rejected_before_ingest_or_verification() {
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::io::Cursor;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use wiki_source::public_dump::DumpChecksum;
    use wiki_source::public_dump_stage::{
        stage_verified_member, verify_staged_export, StageWriteError, StagedDumpError,
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "wiki-dump-root-permissions-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let name = "enwiki-2026-10-01-p1p100.xml.bz2";
    let bytes = b"verified fixture bytes";
    let member = DumpChecksum {
        relative_path: name.into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    };
    fs::set_permissions(&root, fs::Permissions::from_mode(0o770)).unwrap();
    assert!(matches!(
        stage_verified_member(&root, &member, &mut Cursor::new(bytes), 1024),
        Err(StageWriteError::UnsafeStagingPath)
    ));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    stage_verified_member(&root, &member, &mut Cursor::new(bytes), 1024).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o770)).unwrap();
    assert!(matches!(
        verify_staged_export(&root, &[member.clone()], "enwiki", "2026-10-01", 1024),
        Err(StagedDumpError::UnsafeStagingPath)
    ));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        verify_staged_export(&root, &[member], "enwiki", "2026-10-01", 1024).unwrap(),
        bytes.len() as u64
    );
    fs::remove_dir_all(root).unwrap();
}
