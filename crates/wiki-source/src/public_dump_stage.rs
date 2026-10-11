//! Verify every declared public-export member in a private staging directory.
//!
//! The caller must separately authenticate the upstream SHA256SUMS publication.
//! Successful staging verification does not import XML, prove renderer fidelity,
//! or activate a canonical snapshot. The staging tree must not be concurrently
//! writable by an untrusted process (path checks alone cannot defeat TOCTOU).

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::public_dump::{matches_current_export, DumpChecksum};
use crate::public_dump_verify::{verify_dump_member, DumpVerifyError};

#[derive(Debug)]
pub enum StagedDumpError {
    InvalidInventory,
    UnsafeStagingPath,
    TooLarge,
    Io(std::io::Error),
    Member(DumpVerifyError),
}

/// Hash every listed export member before an importer may consume the staging
/// tree. Reject symlink components and non-regular objects. Unlisted files are
/// ignored, not considered part of the verified export.
pub fn verify_staged_export(
    root: &Path,
    members: &[DumpChecksum],
    wiki: &str,
    date: &str,
    max_member_bytes: u64,
) -> Result<u64, StagedDumpError> {
    if !matches_current_export(members, wiki, date)
        || max_member_bytes == 0
        || max_member_bytes > (1u64 << 42)
    {
        return Err(StagedDumpError::InvalidInventory);
    }
    let metadata = fs::symlink_metadata(root).map_err(StagedDumpError::Io)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(StagedDumpError::UnsafeStagingPath);
    }
    let mut total = 0u64;
    for member in members {
        let mut path = PathBuf::from(root);
        let components: Vec<_> = member.relative_path.split('/').collect();
        for (index, component) in components.iter().enumerate() {
            path.push(component);
            let metadata = fs::symlink_metadata(&path).map_err(StagedDumpError::Io)?;
            let is_last = index + 1 == components.len();
            if metadata.file_type().is_symlink()
                || (is_last && !metadata.file_type().is_file())
                || (!is_last && !metadata.file_type().is_dir())
            {
                return Err(StagedDumpError::UnsafeStagingPath);
            }
        }
        let mut file = File::open(&path).map_err(StagedDumpError::Io)?;
        let size = verify_dump_member(&mut file, member, max_member_bytes)
            .map_err(StagedDumpError::Member)?;
        total = total.checked_add(size).ok_or(StagedDumpError::TooLarge)?;
        if total > (1u64 << 44) {
            return Err(StagedDumpError::TooLarge);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-staged-dump-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    fn member(path: &str, bytes: &[u8]) -> DumpChecksum {
        DumpChecksum {
            relative_path: path.into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    #[test]
    fn verifies_all_declared_members_and_rejects_corrupt_or_missing_shards() {
        let root = root();
        let a = b"first official export shard";
        let b = b"second official export shard";
        let first = "enwiki-2026-10-01-p1p100.xml.bz2";
        let second = "enwiki-2026-10-01-p101p200.xml.bz2";
        fs::write(root.join(first), a).unwrap();
        fs::write(root.join(second), b).unwrap();
        let members = vec![member(first, a), member(second, b)];
        assert_eq!(
            verify_staged_export(&root, &members, "enwiki", "2026-10-01", 1024).unwrap(),
            (a.len() + b.len()) as u64
        );
        assert!(matches!(
            verify_staged_export(&root, &members, "frwiki", "2026-10-01", 1024),
            Err(StagedDumpError::InvalidInventory)
        ));
        fs::write(root.join(second), b"corrupted").unwrap();
        assert!(matches!(
            verify_staged_export(&root, &members, "enwiki", "2026-10-01", 1024),
            Err(StagedDumpError::Member(DumpVerifyError::DigestMismatch))
        ));
        fs::remove_file(root.join(first)).unwrap();
        assert!(matches!(
            verify_staged_export(&root, &members, "enwiki", "2026-10-01", 1024),
            Err(StagedDumpError::Io(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_staging_roots_and_member_directories() {
        use std::os::unix::fs::symlink;

        let root = root();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        let member_path = "nested/enwiki-2026-10-01-p1p100.xml.bz2";
        fs::write(outside.join("enwiki-2026-10-01-p1p100.xml.bz2"), b"data").unwrap();
        symlink(&outside, root.join("nested")).unwrap();
        let members = vec![member(member_path, b"data")];
        assert!(matches!(
            verify_staged_export(&root, &members, "enwiki", "2026-10-01", 1024),
            Err(StagedDumpError::UnsafeStagingPath)
        ));
        let root_link = root.with_extension("link");
        symlink(&root, &root_link).unwrap();
        assert!(matches!(
            verify_staged_export(&root_link, &members, "enwiki", "2026-10-01", 1024),
            Err(StagedDumpError::UnsafeStagingPath)
        ));
        fs::remove_file(root_link).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
