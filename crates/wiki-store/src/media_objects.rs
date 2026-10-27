//! Bounded content-addressed media bytes for offline visual assets.
//!
//! This store does not confer publisher trust, validate media licenses, or
//! manage owner lifetimes. The separate ownership registry handles those
//! concerns; installed-pack readiness needs a complete verified manifest.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum MediaObjectError {
    Io(std::io::Error),
    InvalidDigest,
    UnsafeDirectory,
    UnsafeObject,
    ObjectTooLarge,
    DigestMismatch,
    InvalidLimit,
    TooManyObjects,
}

/// The no-overwrite hard-link publisher temporarily holds two links until
/// its private staging name is removed. A concurrent reader must tolerate
/// that short publication window, but reject persistent external hard links.
#[cfg(unix)]
fn require_single_link(file: &File) -> Result<(), MediaObjectError> {
    use std::os::unix::fs::MetadataExt;
    for attempt in 0..50 {
        if file.metadata()?.nlink() == 1 {
            return Ok(());
        }
        if attempt != 49 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    Err(MediaObjectError::UnsafeObject)
}

impl From<std::io::Error> for MediaObjectError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct MediaObjectStore {
    root: PathBuf,
}

impl MediaObjectStore {
    /// The root directory must already exist and must not be a symlink.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn object_path(&self, digest: &str) -> Result<PathBuf, MediaObjectError> {
        // Reject lexical traversal even when the resolved directory happens
        // to be inside the store. A later directory replacement could turn
        // an otherwise harmless ".." component into an escape.
        if self
            .root
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(MediaObjectError::UnsafeDirectory);
        }
        if !valid_digest(digest) {
            return Err(MediaObjectError::InvalidDigest);
        }
        // An apparently ordinary root reached through a symlinked ancestor
        // could escape the caller's designated object-storage directory.
        // This preflight is not race protection against an untrusted writer.
        for ancestor in self.root.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let metadata = fs::symlink_metadata(ancestor)?;
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(MediaObjectError::UnsafeDirectory);
            }
        }
        Ok(self.root.join(&digest[..2]).join(digest))
    }

    /// Enumerate fully published digest paths for a read-only orphan audit.
    /// Staging files are excluded; this snapshot is never deletion authority.
    /// Unexpected paths or aliases fail closed rather than being followed.
    pub fn published_object_digests(
        &self,
        max_objects: usize,
    ) -> Result<Vec<String>, MediaObjectError> {
        if !(1..=10_000).contains(&max_objects) {
            return Err(MediaObjectError::InvalidLimit);
        }
        self.object_path(&"0".repeat(64))?;
        let mut digests = Vec::new();
        let mut staged_count = 0usize;
        for prefix_entry in fs::read_dir(&self.root)? {
            let prefix_entry = prefix_entry?;
            let prefix_name = prefix_entry.file_name();
            let prefix = prefix_name
                .to_str()
                .ok_or(MediaObjectError::UnsafeDirectory)?;
            if prefix.len() != 2
                || !prefix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || !fs::symlink_metadata(prefix_entry.path())?
                    .file_type()
                    .is_dir()
            {
                return Err(MediaObjectError::UnsafeDirectory);
            }
            for entry in fs::read_dir(prefix_entry.path())? {
                let entry = entry?;
                let filename = entry.file_name();
                let name = filename.to_str().ok_or(MediaObjectError::UnsafeObject)?;
                // Interrupted no-overwrite publication can leave private
                // .PID.COUNTER.tmp files; they are not published objects.
                let staging = name
                    .strip_prefix('.')
                    .and_then(|rest| rest.strip_suffix(".tmp"))
                    .and_then(|rest| rest.split_once('.'))
                    .is_some_and(|(pid, counter)| {
                        !pid.is_empty()
                            && !counter.is_empty()
                            && pid.bytes().all(|byte| byte.is_ascii_digit())
                            && counter.bytes().all(|byte| byte.is_ascii_digit())
                    });
                if staging {
                    // A staging-looking symlink or directory must not evade
                    // the path audit. Also bound the number of ignored files:
                    // an unbounded staging flood would make a bounded scan
                    // take arbitrarily long without counting any objects.
                    let metadata = fs::symlink_metadata(entry.path())?;
                    if !metadata.file_type().is_file() {
                        return Err(MediaObjectError::UnsafeObject);
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        // An ignored staging filename must not conceal an
                        // external hard-link alias into the object store.
                        if metadata.nlink() != 1 {
                            return Err(MediaObjectError::UnsafeObject);
                        }
                    }
                    staged_count += 1;
                    if staged_count > max_objects {
                        return Err(MediaObjectError::TooManyObjects);
                    }
                    continue;
                }
                if !valid_digest(name)
                    || !name.starts_with(prefix)
                    || !fs::symlink_metadata(entry.path())?.file_type().is_file()
                {
                    return Err(MediaObjectError::UnsafeObject);
                }
                digests.push(name.to_owned());
                if digests.len() > max_objects {
                    return Err(MediaObjectError::TooManyObjects);
                }
            }
        }
        digests.sort_unstable();
        Ok(digests)
    }

    /// Publish verified bytes with a no-overwrite hard-link promotion. A
    /// concurrent identical writer can reuse the object; conflicting bytes,
    /// unsafe paths, and preexisting symlinks are never silently accepted.
    pub fn store_bytes(&self, bytes: &[u8]) -> Result<String, MediaObjectError> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_OBJECT_BYTES {
            return Err(MediaObjectError::ObjectTooLarge);
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        let path = self.object_path(&digest)?;
        let parent = path.parent().ok_or(MediaObjectError::UnsafeDirectory)?;
        // A second installer may create the same digest-prefix directory
        // between checks. Still verify its type below before writing.
        match fs::create_dir(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(MediaObjectError::Io(error)),
        }
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeDirectory);
        }
        // A process killed before cleanup can leave a temp file behind.
        // PID reuse must not make the next install fail or overwrite that file.
        let (temp, mut output) = loop {
            let candidate = parent.join(format!(
                ".{}.{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(output) => break (candidate, output),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(MediaObjectError::Io(error)),
            }
        };
        let result = (|| -> Result<(), MediaObjectError> {
            output.write_all(bytes)?;
            output.sync_all()?;
            drop(output);
            match fs::hard_link(&temp, &path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if self.read_verified(&digest)? != bytes {
                        return Err(MediaObjectError::DigestMismatch);
                    }
                }
                Err(error) => return Err(MediaObjectError::Io(error)),
            }
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                // The promotion above briefly leaves two hard links to the
                // same inode. Persist removal of the staging name before
                // reporting success, or a crash could leave an apparently
                // permanently hardlinked (and therefore unreadable) object.
                fs::remove_file(&temp)?;
                File::open(parent)?.sync_all()?;
                Ok(digest)
            }
            Err(error) => {
                // Failed installs never remove published bytes. A leftover
                // staging file is conservatively retained if cleanup fails.
                let _ = fs::remove_file(&temp);
                Err(error)
            }
        }
    }

    /// Verify a stored object with bounded memory, returning its byte length.
    /// Pack-wide media audits should not allocate an entire 64 MiB image just
    /// to check its digest. This validates bytes, not provenance or licensing.
    pub fn verify_object_size(&self, digest: &str) -> Result<u64, MediaObjectError> {
        let path = self.object_path(digest)?;
        let parent = path.parent().ok_or(MediaObjectError::UnsafeDirectory)?;
        let directory = fs::symlink_metadata(parent)?;
        if !directory.file_type().is_dir() || directory.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeDirectory);
        }
        let before = fs::symlink_metadata(&path)?;
        if !before.file_type().is_file() || before.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeObject);
        }
        if before.len() == 0 || before.len() > MAX_OBJECT_BYTES {
            return Err(MediaObjectError::ObjectTooLarge);
        }
        let mut file = File::open(&path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() != before.len() {
            return Err(MediaObjectError::UnsafeObject);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if before.ino() != opened.ino() || before.dev() != opened.dev() {
                return Err(MediaObjectError::UnsafeObject);
            }
            require_single_link(&file)?;
        }

        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 8192];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .ok_or(MediaObjectError::ObjectTooLarge)?;
            if total > MAX_OBJECT_BYTES {
                return Err(MediaObjectError::ObjectTooLarge);
            }
            hasher.update(&buffer[..count]);
        }
        let after = file.metadata()?;
        if total != opened.len() || after.len() != opened.len() {
            return Err(MediaObjectError::UnsafeObject);
        }
        #[cfg(unix)]
        {
            require_single_link(&file)?;
        }
        if format!("{:x}", hasher.finalize()) != digest {
            return Err(MediaObjectError::DigestMismatch);
        }
        Ok(total)
    }

    /// Verify a bounded object from its on-disk bytes on every read.
    pub fn read_verified(&self, digest: &str) -> Result<Vec<u8>, MediaObjectError> {
        let path = self.object_path(digest)?;
        let parent = path.parent().ok_or(MediaObjectError::UnsafeDirectory)?;
        let directory = fs::symlink_metadata(parent)?;
        if !directory.file_type().is_dir() || directory.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeDirectory);
        }
        let before = fs::symlink_metadata(&path)?;
        if !before.file_type().is_file() || before.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeObject);
        }
        if before.len() == 0 || before.len() > MAX_OBJECT_BYTES {
            return Err(MediaObjectError::ObjectTooLarge);
        }
        let mut file = File::open(&path)?;
        let opened = file.metadata()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // A second hard link outside the store permits another filename
            // to mutate verified bytes after publication. Reject such objects
            // rather than treating their matching digest as immutable storage.
            if before.ino() != opened.ino() || before.dev() != opened.dev() {
                return Err(MediaObjectError::UnsafeObject);
            }
            require_single_link(&file)?;
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(MAX_OBJECT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        if bytes.len() as u64 != opened.len() || after.len() != opened.len() {
            return Err(MediaObjectError::UnsafeObject);
        }
        #[cfg(unix)]
        {
            require_single_link(&file)?;
        }
        if format!("{:x}", Sha256::digest(&bytes)) != digest {
            return Err(MediaObjectError::DigestMismatch);
        }
        Ok(bytes)
    }
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-objects-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn lexical_parent_traversal_is_rejected_for_reads_writes_and_scans() {
        let root = root();
        fs::create_dir(root.join("child")).unwrap();
        let valid = MediaObjectStore::new(&root);
        let digest = valid.store_bytes(b"valid object").unwrap();
        let traversal = MediaObjectStore::new(root.join("child").join(".."));
        assert!(matches!(
            traversal.store_bytes(b"new object"),
            Err(MediaObjectError::UnsafeDirectory)
        ));
        assert!(matches!(
            traversal.read_verified(&digest),
            Err(MediaObjectError::UnsafeDirectory)
        ));
        assert!(matches!(
            traversal.published_object_digests(10),
            Err(MediaObjectError::UnsafeDirectory)
        ));
        assert_eq!(valid.read_verified(&digest).unwrap(), b"valid object");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn successful_promotion_removes_staging_hardlink_before_returning() {
        use std::os::unix::fs::MetadataExt;

        let root = root();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"durable figure").unwrap();
        let parent = root.join(&digest[..2]);
        let object = parent.join(&digest);
        assert_eq!(fs::metadata(&object).unwrap().nlink(), 1);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        assert_eq!(store.store_bytes(b"durable figure").unwrap(), digest);
        assert_eq!(fs::metadata(&object).unwrap().nlink(), 1);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        assert_eq!(store.read_verified(&digest).unwrap(), b"durable figure");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_bytes_dedupe_and_tampering_is_rejected() {
        let root = root();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"diagram").unwrap();
        assert_eq!(store.store_bytes(b"diagram").unwrap(), digest);
        assert_eq!(store.read_verified(&digest).unwrap(), b"diagram");
        fs::write(root.join(&digest[..2]).join(&digest), b"tampered").unwrap();
        assert!(matches!(
            store.read_verified(&digest),
            Err(MediaObjectError::DigestMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abandoned_temporary_files_do_not_block_publishing() {
        let root = root();
        let store = MediaObjectStore::new(&root);
        let bytes = b"recover after interrupted write";
        let digest = format!("{:x}", Sha256::digest(bytes));
        let parent = root.join(&digest[..2]);
        fs::create_dir(&parent).unwrap();
        // Cover the next several counter values even if tests run in parallel.
        let next = NEXT_TEMP.load(Ordering::Relaxed);
        let abandoned: Vec<_> = (next..next + 32)
            .map(|index| parent.join(format!(".{}.{}.tmp", std::process::id(), index)))
            .collect();
        for path in &abandoned {
            fs::write(path, b"orphaned partial payload").unwrap();
        }
        assert_eq!(store.store_bytes(bytes).unwrap(), digest);
        assert_eq!(store.read_verified(&digest).unwrap(), bytes);
        for path in &abandoned {
            assert_eq!(fs::read(path).unwrap(), b"orphaned partial payload");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_identical_writers_share_one_verified_object() {
        use std::sync::{Arc, Barrier};

        let root = root();
        let store = Arc::new(MediaObjectStore::new(&root));
        const WRITERS: usize = 12;
        let start = Arc::new(Barrier::new(WRITERS));
        let threads: Vec<_> = (0..WRITERS)
            .map(|_| {
                let store = Arc::clone(&store);
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    store.store_bytes(b"shared scientific diagram").unwrap()
                })
            })
            .collect();
        let digests: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        for digest in &digests {
            assert_eq!(digest, &digests[0]);
            assert_eq!(
                store.read_verified(digest).unwrap(),
                b"shared scientific diagram"
            );
        }
        let prefix = root.join(&digests[0][..2]);
        assert_eq!(
            fs::read_dir(prefix).unwrap().filter_map(Result::ok).count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn published_scan_bounds_ignored_staging_files_without_removing_them() {
        let root = root();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"published visual").unwrap();
        let prefix = root.join(&digest[..2]);
        let first = prefix.join(".123.456.tmp");
        let second = prefix.join(".123.457.tmp");
        fs::write(&first, b"partial one").unwrap();
        fs::write(&second, b"partial two").unwrap();

        assert!(matches!(
            store.published_object_digests(1),
            Err(MediaObjectError::TooManyObjects)
        ));
        assert_eq!(store.published_object_digests(2).unwrap(), vec![digest]);
        assert_eq!(fs::read(first).unwrap(), b"partial one");
        assert_eq!(fs::read(second).unwrap(), b"partial two");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn published_scan_rejects_hardlinked_staging_aliases() {
        let root = root();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"published figure").unwrap();
        let staging = root.join(&digest[..2]).join(".123.456.tmp");
        let outside = root.with_extension("outside-staging-hardlink");
        fs::write(&outside, b"external bytes").unwrap();
        fs::hard_link(&outside, &staging).unwrap();
        assert!(matches!(
            store.published_object_digests(10),
            Err(MediaObjectError::UnsafeObject)
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"external bytes");
        fs::remove_file(&staging).unwrap();
        assert_eq!(store.published_object_digests(10).unwrap(), vec![digest]);
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn published_scan_rejects_staging_symlinks_and_directories() {
        use std::os::unix::fs::symlink;

        let root = root();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"published visual").unwrap();
        let prefix = root.join(&digest[..2]);
        let staged = prefix.join(".123.456.tmp");
        let target = root.with_extension("external-stage");
        fs::write(&target, b"outside").unwrap();
        symlink(&target, &staged).unwrap();
        assert!(matches!(
            store.published_object_digests(10),
            Err(MediaObjectError::UnsafeObject)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"outside");
        fs::remove_file(&staged).unwrap();

        fs::create_dir(&staged).unwrap();
        assert!(matches!(
            store.published_object_digests(10),
            Err(MediaObjectError::UnsafeObject)
        ));
        fs::remove_dir(&staged).unwrap();
        assert_eq!(store.published_object_digests(10).unwrap(), vec![digest]);
        fs::remove_file(target).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_digest_directory_is_rejected_before_writing() {
        use std::os::unix::fs::symlink;

        let root = root();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        let bytes = b"must not follow a symlink";
        let digest = format!("{:x}", Sha256::digest(bytes));
        symlink(&outside, root.join(&digest[..2])).unwrap();
        let store = MediaObjectStore::new(&root);
        assert!(matches!(
            store.store_bytes(bytes),
            Err(MediaObjectError::UnsafeDirectory)
        ));
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_ancestor_cannot_escape_media_store() {
        use std::os::unix::fs::symlink;

        let root = root();
        let canonical = root.join("canonical");
        let nested = canonical.join("nested");
        fs::create_dir_all(&nested).unwrap();
        let alias = root.join("alias");
        symlink(&canonical, &alias).unwrap();

        let valid = MediaObjectStore::new(&nested);
        let digest = valid.store_bytes(b"figure").unwrap();
        assert_eq!(valid.read_verified(&digest).unwrap(), b"figure");

        // The leaf 'nested' is a real directory, but its parent is not.
        let through_alias = MediaObjectStore::new(alias.join("nested"));
        assert!(matches!(
            through_alias.store_bytes(b"another figure"),
            Err(MediaObjectError::UnsafeDirectory)
        ));
        assert!(matches!(
            through_alias.read_verified(&digest),
            Err(MediaObjectError::UnsafeDirectory)
        ));
        assert_eq!(valid.read_verified(&digest).unwrap(), b"figure");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn externally_hardlinked_object_is_not_accepted_as_immutable() {
        let root = root();
        let store = MediaObjectStore::new(&root);
        let content = b"offline diagram cannot be externally mutable";
        let digest = store.store_bytes(content).unwrap();
        let object = root.join(&digest[..2]).join(&digest);
        let outside = root.with_extension("outside-hardlink");
        fs::hard_link(&object, &outside).unwrap();

        // The digest still matches, but an outside filename can rewrite the
        // inode, so neither reads nor deduplicated installs may accept it.
        assert!(matches!(
            store.read_verified(&digest),
            Err(MediaObjectError::UnsafeObject)
        ));
        assert!(matches!(
            store.store_bytes(content),
            Err(MediaObjectError::UnsafeObject)
        ));
        assert_eq!(fs::read(&outside).unwrap(), content);
        fs::remove_file(&outside).unwrap();
        assert_eq!(store.read_verified(&digest).unwrap(), content);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_object_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = root();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"figure").unwrap();
        let path = root.join(&digest[..2]).join(&digest);
        fs::remove_file(&path).unwrap();
        symlink(root.join("missing"), &path).unwrap();
        assert!(matches!(
            store.read_verified(&digest),
            Err(MediaObjectError::UnsafeObject)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
