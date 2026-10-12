//! Verify every declared public-export member in a private staging directory.
//!
//! The caller must separately authenticate the upstream SHA256SUMS publication.
//! Successful staging verification does not import XML, prove renderer fidelity,
//! or activate a canonical snapshot. The staging tree must not be concurrently
//! writable by an untrusted process (path checks alone cannot defeat TOCTOU).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::public_dump::{matches_current_export, parse_sha256sums, DumpChecksum};
use crate::public_dump_verify::{verify_dump_member, DumpVerifyError};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// A regular staging root is insufficient when an ancestor is a symlink:
/// opening root/member would still follow that alias into a different tree.
/// This is a preflight check; the caller must also prevent concurrent renames.
fn has_symlink_ancestor(root: &Path) -> std::io::Result<bool> {
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

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
    if has_symlink_ancestor(root).map_err(StagedDumpError::Io)? {
        return Err(StagedDumpError::UnsafeStagingPath);
    }
    let metadata = fs::symlink_metadata(root).map_err(StagedDumpError::Io)?;
    if !metadata.file_type().is_dir() {
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
            #[cfg(unix)]
            if is_last {
                use std::os::unix::fs::MetadataExt;
                // A checksum-valid hard link into a mutable outside location
                // is not an isolated staged source.
                if metadata.nlink() != 1 {
                    return Err(StagedDumpError::UnsafeStagingPath);
                }
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

#[derive(Debug)]
pub enum StageWriteError {
    InvalidMember,
    UnsafeStagingPath,
    TooLarge,
    EmptyMember,
    DigestMismatch,
    Io(std::io::Error),
    Existing(DumpVerifyError),
}

/// Stream a checksum-declared member into a private staging tree without
/// publishing partial bytes. A no-overwrite hard link makes repeated staging
/// idempotent only when the existing member also passes digest verification.
/// This is not a network downloader or an upstream authentication check.
pub fn stage_verified_member<R: Read>(
    root: &Path,
    member: &DumpChecksum,
    source: &mut R,
    max_bytes: u64,
) -> Result<u64, StageWriteError> {
    let line = format!("{}  {}", member.sha256, member.relative_path);
    if parse_sha256sums(&line).is_err() {
        return Err(StageWriteError::InvalidMember);
    }
    if max_bytes == 0 || max_bytes > (1u64 << 42) {
        return Err(StageWriteError::TooLarge);
    }
    if has_symlink_ancestor(root).map_err(StageWriteError::Io)? {
        return Err(StageWriteError::UnsafeStagingPath);
    }
    let metadata = fs::symlink_metadata(root).map_err(StageWriteError::Io)?;
    if !metadata.file_type().is_dir() {
        return Err(StageWriteError::UnsafeStagingPath);
    }
    let mut parent = root.to_path_buf();
    let mut parts = member.relative_path.split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            break;
        }
        parent.push(part);
        #[cfg(unix)]
        let created = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&parent)
        };
        #[cfg(not(unix))]
        let created = fs::create_dir(&parent);
        match created {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(StageWriteError::Io(error)),
        }
        let metadata = fs::symlink_metadata(&parent).map_err(StageWriteError::Io)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(StageWriteError::UnsafeStagingPath);
        }
    }
    let target = root.join(&member.relative_path);
    let (temp, mut output) = loop {
        let candidate = parent.join(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(output) => break (candidate, output),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(StageWriteError::Io(error)),
        }
    };
    let result = (|| -> Result<u64, StageWriteError> {
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let n = match source.read(&mut chunk) {
                Ok(n) => n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(StageWriteError::Io(error)),
            };
            if n == 0 {
                break;
            }
            total = total
                .checked_add(n as u64)
                .ok_or(StageWriteError::TooLarge)?;
            if total > max_bytes {
                return Err(StageWriteError::TooLarge);
            }
            hash.update(&chunk[..n]);
            output.write_all(&chunk[..n]).map_err(StageWriteError::Io)?;
        }
        if total == 0 {
            return Err(StageWriteError::EmptyMember);
        }
        if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&member.sha256) {
            return Err(StageWriteError::DigestMismatch);
        }
        output.sync_all().map_err(StageWriteError::Io)?;
        drop(output);
        match fs::hard_link(&temp, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&target).map_err(StageWriteError::Io)?;
                if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                    return Err(StageWriteError::UnsafeStagingPath);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    // Existing published members must not alias mutable
                    // files outside the private staging tree.
                    if metadata.nlink() != 1 {
                        return Err(StageWriteError::UnsafeStagingPath);
                    }
                }
                let mut existing = File::open(&target).map_err(StageWriteError::Io)?;
                let size = verify_dump_member(&mut existing, member, max_bytes)
                    .map_err(StageWriteError::Existing)?;
                if size != total {
                    return Err(StageWriteError::DigestMismatch);
                }
            }
            Err(error) => return Err(StageWriteError::Io(error)),
        }
        #[cfg(unix)]
        File::open(&parent)
            .and_then(|dir| dir.sync_all())
            .map_err(StageWriteError::Io)?;
        Ok(total)
    })();
    let _ = fs::remove_file(&temp);
    result
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
        let root =
            std::env::temp_dir().join(format!("wiki-staged-dump-{}-{stamp}", std::process::id()));
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

    #[test]
    fn streamed_staging_publishes_only_complete_verified_members() {
        use std::io::Cursor;

        let root = root();
        let path = "nested/enwiki-2026-10-01-p1p100.xml.bz2";
        let bytes = b"verified current-content shard";
        let descriptor = member(path, bytes);
        let target = root.join(path);
        assert!(matches!(
            stage_verified_member(&root, &descriptor, &mut Cursor::new(b"tampered"), 1024),
            Err(StageWriteError::DigestMismatch)
        ));
        assert!(!target.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(
            stage_verified_member(&root, &descriptor, &mut Cursor::new(bytes), 1024).unwrap(),
            bytes.len() as u64
        );
        assert_eq!(fs::read(&target).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(target.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert_eq!(
            stage_verified_member(&root, &descriptor, &mut Cursor::new(bytes), 1024).unwrap(),
            bytes.len() as u64
        );
        assert!(matches!(
            stage_verified_member(&root, &descriptor, &mut Cursor::new(bytes), 4),
            Err(StageWriteError::TooLarge)
        ));
        assert_eq!(fs::read_dir(root.join("nested")).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn verified_staging_rejects_hardlinked_external_member() {
        use std::io::Cursor;

        let root = root();
        let outside = root.with_extension("outside");
        let bytes = b"valid content from a mutable outside file";
        fs::write(&outside, bytes).unwrap();
        let name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let target = root.join(name);
        fs::hard_link(&outside, &target).unwrap();
        let descriptor = member(name, bytes);

        assert!(matches!(
            verify_staged_export(&root, &[descriptor.clone()], "enwiki", "2026-10-01", 1024),
            Err(StagedDumpError::UnsafeStagingPath)
        ));
        assert!(matches!(
            stage_verified_member(&root, &descriptor, &mut Cursor::new(bytes), 1024),
            Err(StageWriteError::UnsafeStagingPath)
        ));
        assert_eq!(fs::read(&outside).unwrap(), bytes);
        assert_eq!(fs::read(&target).unwrap(), bytes);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn streamed_staging_rejects_symlinked_parent_and_existing_target() {
        use std::io::Cursor;
        use std::os::unix::fs::symlink;

        let root = root();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, root.join("nested")).unwrap();
        let descriptor = member("nested/enwiki-2026-10-01-p1p100.xml.bz2", b"data");
        assert!(matches!(
            stage_verified_member(&root, &descriptor, &mut Cursor::new(b"data"), 1024),
            Err(StageWriteError::UnsafeStagingPath)
        ));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
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
    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_ancestor_of_otherwise_regular_staging_root() {
        use std::io::Cursor;
        use std::os::unix::fs::symlink;

        let parent = root();
        let direct = parent.join("staging");
        fs::create_dir(&direct).unwrap();
        let alias = parent.with_extension("alias");
        symlink(&parent, &alias).unwrap();
        let aliased_root = alias.join("staging");
        let name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let bytes = b"same verified bytes";
        let descriptor = member(name, bytes);
        fs::write(direct.join(name), bytes).unwrap();

        // Even though the final staging directory is regular, its parent
        // alias would redirect reads/writes outside the requested path.
        assert!(matches!(
            verify_staged_export(
                &aliased_root,
                &[descriptor.clone()],
                "enwiki",
                "2026-10-01",
                1024
            ),
            Err(StagedDumpError::UnsafeStagingPath)
        ));
        assert!(matches!(
            stage_verified_member(&aliased_root, &descriptor, &mut Cursor::new(bytes), 1024),
            Err(StageWriteError::UnsafeStagingPath)
        ));
        // A direct private path to the identical bytes is still valid.
        assert_eq!(
            verify_staged_export(&direct, &[descriptor], "enwiki", "2026-10-01", 1024).unwrap(),
            bytes.len() as u64
        );
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(parent).unwrap();
    }
}
