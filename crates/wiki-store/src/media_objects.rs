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
        if !valid_digest(digest) {
            return Err(MediaObjectError::InvalidDigest);
        }
        let root = fs::symlink_metadata(&self.root)?;
        if !root.file_type().is_dir() || root.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeDirectory);
        }
        Ok(self.root.join(&digest[..2]).join(digest))
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
        if !parent.exists() {
            fs::create_dir(parent)?;
        }
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(MediaObjectError::UnsafeDirectory);
        }
        let temp = parent.join(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), MediaObjectError> {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
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
        let _ = fs::remove_file(&temp);
        result?;
        Ok(digest)
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
            if before.ino() != opened.ino() || before.dev() != opened.dev() {
                return Err(MediaObjectError::UnsafeObject);
            }
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(MAX_OBJECT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != opened.len() || file.metadata()?.len() != opened.len() {
            return Err(MediaObjectError::UnsafeObject);
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
