//! Resumable host-side staged Wikimedia dump members.
//!
//! A partial file is intentionally never an installable or verified source.
//! Every HTTP range must match the exact pinned origin and byte offset, and
//! the complete assembled body must pass its independently pinned SHA-256
//! before stage_pinned_member can atomically publish a completed shard.
//! The staging directory must remain private from concurrent untrusted writes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::public_dump_https::{
    verified_existing_member, InventoryCurl, MemberFetchError, RangeFetchError, VerifiedRange,
    MAX_RANGE_BYTES,
};
use crate::public_dump_pinned::{stage_pinned_member, PinnedInventory};
use crate::public_dump_stage::StageWriteError;

#[derive(Debug)]
pub enum ResumeError {
    InvalidMember,
    InvalidLimit,
    UnsafeStaging,
    InvalidRange,
    TotalChanged,
    Transport(RangeFetchError),
    Cache(MemberFetchError),
    Stage(StageWriteError),
    Io(io::Error),
}

fn private_root(root: &Path) -> Result<(), ResumeError> {
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| ResumeError::UnsafeStaging)?;
        if metadata.file_type().is_symlink() {
            return Err(ResumeError::UnsafeStaging);
        }
    }
    let metadata = fs::symlink_metadata(root).map_err(|_| ResumeError::UnsafeStaging)?;
    if !metadata.is_dir() {
        return Err(ResumeError::UnsafeStaging);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(ResumeError::UnsafeStaging);
        }
    }
    Ok(())
}

/// Derive a deterministic partial filename from the independently digest-
/// pinned inventory and exact member. A new generation cannot resume another
/// generation's partial file, even if its member basename is the same.
fn partial_path(root: &Path, inventory: &PinnedInventory, index: usize) -> PathBuf {
    let member = &inventory.members()[index];
    let identity = format!(
        "{}:{}:{}:{}:{}",
        inventory.project(),
        inventory.date(),
        inventory.inventory_sha256(),
        index,
        member.sha256
    );
    root.join(format!(
        ".fw-resume-{:x}.part",
        Sha256::digest(identity.as_bytes())
    ))
}

/// A crash after publishing a fully verified member can leave its private
/// checkpoint behind. Reclaim only an ordinary, isolated private file; a
/// suspicious alias is not ours to remove. Cleanup is opportunistic and
/// cannot turn a verified completed member into a failed cache hit.
fn cleanup_published_checkpoint(root: &Path, inventory: &PinnedInventory, index: usize) {
    let path = partial_path(root, inventory, index);
    let Ok(inspected) = fs::symlink_metadata(&path) else {
        return;
    };
    if !inspected.file_type().is_file() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if inspected.nlink() != 1 || inspected.permissions().mode() & 0o077 != 0 {
            return;
        }
    }
    let Ok(file) = File::open(&path) else {
        return;
    };
    let Ok(opened) = file.metadata() else {
        return;
    };
    if !opened.is_file() || opened.len() != inspected.len() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != inspected.dev() || opened.ino() != inspected.ino() || opened.nlink() != 1 {
            return;
        }
    }
    drop(file);
    if fs::remove_file(&path).is_ok() {
        #[cfg(unix)]
        {
            let _ = File::open(root).and_then(|directory| directory.sync_all());
        }
    }
}

fn open_private_partial(path: &Path, max_bytes: u64) -> Result<File, ResumeError> {
    let inspected = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut options = OpenOptions::new();
            options.create_new(true).write(true).read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            return options.open(path).map_err(ResumeError::Io);
        }
        Err(error) => return Err(ResumeError::Io(error)),
    };
    if !inspected.file_type().is_file() || inspected.len() > max_bytes {
        return Err(ResumeError::UnsafeStaging);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if inspected.nlink() != 1 || inspected.permissions().mode() & 0o077 != 0 {
            return Err(ResumeError::UnsafeStaging);
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .append(true)
        .open(path)
        .map_err(ResumeError::Io)?;
    let opened = file.metadata().map_err(ResumeError::Io)?;
    if !opened.is_file() || opened.len() != inspected.len() {
        return Err(ResumeError::UnsafeStaging);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != inspected.dev() || opened.ino() != inspected.ino() || opened.nlink() != 1
        {
            return Err(ResumeError::UnsafeStaging);
        }
    }
    Ok(file)
}

fn publish_full_partial(
    root: &Path,
    inventory: &PinnedInventory,
    index: usize,
    partial: &Path,
    max_bytes: u64,
) -> Result<u64, ResumeError> {
    let mut source = File::open(partial).map_err(ResumeError::Io)?;
    let bytes = stage_pinned_member(root, inventory, index, &mut source, max_bytes)
        .map_err(ResumeError::Stage)?;
    drop(source);
    fs::remove_file(partial).map_err(ResumeError::Io)?;
    #[cfg(unix)]
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(ResumeError::Io)?;
    Ok(bytes)
}

/// Fetch and persist complete HTTP 206 chunks across separate invocations.
/// A failed chunk is not appended; previous successful chunks remain on disk.
/// Resuming from a disk prefix never asserts that prefix's authenticity:
/// every byte is rehashed against the full declared member checksum at the
/// publication gate. Existing published members are independently rehashed.
pub fn stage_resumed_member_with<F>(
    inventory: &PinnedInventory,
    member_index: usize,
    root: &Path,
    max_bytes: u64,
    chunk_bytes: usize,
    mut fetch: F,
) -> Result<u64, ResumeError>
where
    F: FnMut(u64, usize, u64) -> Result<VerifiedRange, RangeFetchError>,
{
    if max_bytes == 0
        || max_bytes > (1_u64 << 40)
        || chunk_bytes == 0
        || chunk_bytes > MAX_RANGE_BYTES
    {
        return Err(ResumeError::InvalidLimit);
    }
    if inventory.members().get(member_index).is_none() {
        return Err(ResumeError::InvalidMember);
    }
    private_root(root)?;
    if let Some(bytes) = verified_existing_member(inventory, member_index, root, max_bytes)
        .map_err(ResumeError::Cache)?
    {
        cleanup_published_checkpoint(root, inventory, member_index);
        return Ok(bytes);
    }
    let partial = partial_path(root, inventory, member_index);
    let mut output = open_private_partial(&partial, max_bytes)?;
    let mut offset = output.metadata().map_err(ResumeError::Io)?.len();
    // Do not copy and hash a potentially terabyte-sized incomplete prefix
    // into a throwaway publication candidate on every retry. Only attempt
    // full publication when the HTTP server confirms the prefix reached EOF
    // (416) or the caller's maximum byte budget is already exhausted.
    if offset == max_bytes {
        drop(output);
        let published = publish_full_partial(root, inventory, member_index, &partial, max_bytes);
        if matches!(
            published,
            Err(ResumeError::Stage(StageWriteError::DigestMismatch))
        ) {
            let _ = fs::remove_file(&partial);
        }
        return published;
    }
    let mut total = None;
    loop {
        if offset >= max_bytes {
            return Err(ResumeError::InvalidRange);
        }
        let length = (max_bytes - offset).min(chunk_bytes as u64) as usize;
        let response = match fetch(offset, length, max_bytes) {
            Ok(response) => response,
            Err(RangeFetchError::HttpStatus(416)) if offset > 0 => {
                // A fully synced partial may survive a crash immediately
                // before atomic publication. Only its full pinned digest,
                // never HTTP 416 alone, can establish completion.
                drop(output);
                let published =
                    publish_full_partial(root, inventory, member_index, &partial, max_bytes);
                if matches!(
                    published,
                    Err(ResumeError::Stage(StageWriteError::DigestMismatch))
                ) {
                    let _ = fs::remove_file(&partial);
                }
                return published;
            }
            Err(error) => return Err(ResumeError::Transport(error)),
        };
        if response.total_bytes <= offset
            || response.total_bytes > max_bytes
            || response.bytes.is_empty()
            || response.bytes.len() > length
        {
            return Err(ResumeError::InvalidRange);
        }
        if total.is_some_and(|previous| previous != response.total_bytes) {
            return Err(ResumeError::TotalChanged);
        }
        total = Some(response.total_bytes);
        let next = offset
            .checked_add(response.bytes.len() as u64)
            .filter(|next| *next <= response.total_bytes)
            .ok_or(ResumeError::InvalidRange)?;
        output.write_all(&response.bytes).map_err(ResumeError::Io)?;
        output.sync_all().map_err(ResumeError::Io)?;
        offset = next;
        if offset == response.total_bytes {
            break;
        }
    }
    drop(output);
    let published = publish_full_partial(root, inventory, member_index, &partial, max_bytes);
    if matches!(
        published,
        Err(ResumeError::Stage(StageWriteError::DigestMismatch))
    ) {
        // A pinned checksum failure is not a resumable transient; remove the
        // poisoned prefix to allow a subsequent clean download.
        let _ = fs::remove_file(&partial);
    }
    published
}

impl InventoryCurl {
    /// Host-only resumable acquisition using fixed-size authenticated range
    /// metadata, private partial staging and mandatory whole-shard SHA-256.
    pub fn stage_resumed_member(
        &self,
        inventory: &PinnedInventory,
        member_index: usize,
        root: &Path,
        max_bytes: u64,
    ) -> Result<u64, ResumeError> {
        stage_resumed_member_with(
            inventory,
            member_index,
            root,
            max_bytes,
            MAX_RANGE_BYTES,
            |start, length, budget| {
                self.fetch_member_range(inventory, member_index, start, length, budget)
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> (PathBuf, PinnedInventory) {
        let bytes = b"abcdef";
        let filename = "enwiki-2026-10-01-p1p100.xml.bz2";
        let sums = format!("{:x}  {filename}\n", Sha256::digest(bytes));
        let pin = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let inventory =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &pin).unwrap();
        let root = std::env::temp_dir().join(format!(
            "wiki-resume-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        (root, inventory)
    }

    #[test]
    fn interrupted_range_continues_from_synced_prefix_and_publishes_only_after_sha256() {
        let (root, inventory) = fixture();
        let partial = partial_path(&root, &inventory, 0);
        let result = stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
            if start == 0 {
                return Ok(VerifiedRange {
                    total_bytes: 6,
                    bytes: b"abc".to_vec(),
                });
            }
            Err(RangeFetchError::IoFailure)
        });
        assert!(matches!(
            result,
            Err(ResumeError::Transport(RangeFetchError::IoFailure))
        ));
        assert_eq!(fs::read(&partial).unwrap(), b"abc");
        assert!(!root.join("enwiki-2026-10-01-p1p100.xml.bz2").exists());

        let mut offsets = Vec::new();
        assert_eq!(
            stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
                offsets.push(start);
                Ok(VerifiedRange {
                    total_bytes: 6,
                    bytes: b"def".to_vec(),
                })
            })
            .unwrap(),
            6
        );
        assert_eq!(offsets, [3]);
        assert_eq!(
            fs::read(root.join("enwiki-2026-10-01-p1p100.xml.bz2")).unwrap(),
            b"abcdef"
        );
        assert!(!partial.exists());

        assert_eq!(
            stage_resumed_member_with(&inventory, 0, &root, 64, 3, |_, _, _| {
                panic!("verified completed member must be reusable offline")
            })
            .unwrap(),
            6
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resumed_prefix_is_not_copied_until_complete_and_416_only_recovers_valid_digest() {
        let (root, inventory) = fixture();
        let partial = partial_path(&root, &inventory, 0);
        fs::write(&partial, b"abc").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o600)).unwrap();
        let mut attempts = Vec::new();
        assert_eq!(
            stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
                attempts.push(start);
                Ok(VerifiedRange {
                    total_bytes: 6,
                    bytes: b"def".to_vec(),
                })
            })
            .unwrap(),
            6
        );
        assert_eq!(attempts, [3]);
        assert!(!partial.exists());

        fs::remove_file(root.join("enwiki-2026-10-01-p1p100.xml.bz2")).unwrap();
        fs::write(&partial, b"abcdef").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
                assert_eq!(start, 6);
                Err(RangeFetchError::HttpStatus(416))
            })
            .unwrap(),
            6
        );
        assert!(!partial.exists());

        fs::remove_file(root.join("enwiki-2026-10-01-p1p100.xml.bz2")).unwrap();
        fs::write(&partial, b"abcxyz").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
                assert_eq!(start, 6);
                Err(RangeFetchError::HttpStatus(416))
            }),
            Err(ResumeError::Stage(StageWriteError::DigestMismatch))
        ));
        assert!(!partial.exists());
        assert!(!root.join("enwiki-2026-10-01-p1p100.xml.bz2").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_cache_hit_reclaims_orphaned_private_checkpoint_without_fetching() {
        let (root, inventory) = fixture();
        let final_name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let partial = partial_path(&root, &inventory, 0);
        fs::write(root.join(final_name), b"abcdef").unwrap();
        fs::write(&partial, b"ab").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o600)).unwrap();

        assert_eq!(
            stage_resumed_member_with(&inventory, 0, &root, 64, 3, |_, _, _| {
                panic!("completed verified shard must not issue HTTP requests")
            })
            .unwrap(),
            6
        );
        assert!(!partial.exists());
        assert_eq!(fs::read(root.join(final_name)).unwrap(), b"abcdef");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let external = root.with_extension("external");
            fs::write(&external, b"outside").unwrap();
            symlink(&external, &partial).unwrap();
            assert_eq!(
                stage_resumed_member_with(&inventory, 0, &root, 64, 3, |_, _, _| {
                    panic!("cached shard must not issue HTTP requests")
                })
                .unwrap(),
                6
            );
            assert!(fs::symlink_metadata(&partial).unwrap().file_type().is_symlink());
            assert_eq!(fs::read(&external).unwrap(), b"outside");
            fs::remove_file(&partial).unwrap();

            fs::hard_link(&external, &partial).unwrap();
            assert_eq!(
                stage_resumed_member_with(&inventory, 0, &root, 64, 3, |_, _, _| {
                    panic!("cached shard must not issue HTTP requests")
                })
                .unwrap(),
                6
            );
            assert_eq!(fs::read(&partial).unwrap(), b"outside");
            fs::remove_file(&external).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn full_checksum_mismatch_cannot_publish_and_clears_poisoned_prefix() {
        let (root, inventory) = fixture();
        let partial = partial_path(&root, &inventory, 0);
        let result = stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
            if start == 0 {
                Ok(VerifiedRange {
                    total_bytes: 6,
                    bytes: b"abc".to_vec(),
                })
            } else {
                Ok(VerifiedRange {
                    total_bytes: 6,
                    bytes: b"xyz".to_vec(),
                })
            }
        });
        assert!(matches!(
            result,
            Err(ResumeError::Stage(StageWriteError::DigestMismatch))
        ));
        assert!(!partial.exists());
        assert!(!root.join("enwiki-2026-10-01-p1p100.xml.bz2").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inconsistent_total_and_symlink_partial_fail_closed_without_publication() {
        let (root, inventory) = fixture();
        let partial = partial_path(&root, &inventory, 0);
        let result = stage_resumed_member_with(&inventory, 0, &root, 64, 3, |start, _, _| {
            if start == 0 {
                Ok(VerifiedRange {
                    total_bytes: 6,
                    bytes: b"abc".to_vec(),
                })
            } else {
                Ok(VerifiedRange {
                    total_bytes: 7,
                    bytes: b"def".to_vec(),
                })
            }
        });
        assert!(matches!(result, Err(ResumeError::TotalChanged)));
        assert_eq!(fs::read(&partial).unwrap(), b"abc");
        fs::remove_file(&partial).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = root.with_extension("external");
            fs::write(&outside, b"abc").unwrap();
            symlink(&outside, &partial).unwrap();
            assert!(matches!(
                stage_resumed_member_with(&inventory, 0, &root, 64, 3, |_, _, _| {
                    panic!("linked partial must be rejected before transfer")
                }),
                Err(ResumeError::UnsafeStaging)
            ));
            fs::remove_file(&partial).unwrap();
            fs::remove_file(outside).unwrap();
        }
        assert!(!root.join("enwiki-2026-10-01-p1p100.xml.bz2").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
