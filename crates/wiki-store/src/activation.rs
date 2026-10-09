//! Crash-conscious active snapshot pointer; partial STORE-003.
//!
//! This is only the atomic SWITCH primitive. Callers must independently
//! validate every snapshot byte, manifest, and schema before invoking it.
//! An existing manifest alone does not establish upstream authenticity.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum ActivationError {
    Io(io::Error),
    UnsafeRoot,
    InvalidSnapshotId,
    CandidateNotStaged,
    CorruptCurrentPointer,
}

impl From<io::Error> for ActivationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug)]
pub struct ActiveSnapshotStore {
    root: PathBuf,
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}

fn real_directory(path: &Path) -> Result<bool, io::Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_dir() && !meta.file_type().is_symlink()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn real_file(path: &Path) -> Result<bool, io::Error> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_file() && !meta.file_type().is_symlink()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

impl ActiveSnapshotStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn ensure_root(&self) -> Result<(), ActivationError> {
        if !real_directory(&self.root)? || !real_directory(&self.root.join("snapshots"))? {
            return Err(ActivationError::UnsafeRoot);
        }
        Ok(())
    }

    fn candidate_is_staged(&self, id: &str) -> Result<bool, ActivationError> {
        let candidate = self.root.join("snapshots").join(id);
        Ok(real_directory(&candidate)? && real_file(&candidate.join("manifest.json"))?)
    }

    pub fn current(&self) -> Result<Option<String>, ActivationError> {
        self.ensure_root()?;
        let pointer = self.root.join("current");
        if !real_file(&pointer)? {
            // Path::exists follows symlinks; a dangling symlink must never
            // masquerade as a missing (uninitialized) current pointer.
            if fs::symlink_metadata(&pointer).is_ok() {
                return Err(ActivationError::CorruptCurrentPointer);
            }
            return Ok(None);
        }
        let id = fs::read_to_string(&pointer)?;
        let id = id.strip_suffix('\n').unwrap_or(&id);
        if !safe_id(id) || !self.candidate_is_staged(id)? {
            return Err(ActivationError::CorruptCurrentPointer);
        }
        Ok(Some(id.to_owned()))
    }

    /// Atomically select a PRE-VERIFIED immutable candidate on the same
    /// filesystem. Missing/unsafe candidates cannot replace the old pointer.
    /// The previous snapshot directory is never deleted by this primitive.
    pub fn switch_to_prevalidated(&self, id: &str) -> Result<(), ActivationError> {
        self.switch_with_precommit_gate(id, || Ok(()))
    }

    fn switch_with_precommit_gate(
        &self,
        id: &str,
        precommit: impl FnOnce() -> Result<(), ActivationError>,
    ) -> Result<(), ActivationError> {
        if !safe_id(id) {
            return Err(ActivationError::InvalidSnapshotId);
        }
        self.ensure_root()?;
        if !self.candidate_is_staged(id)? {
            return Err(ActivationError::CandidateNotStaged);
        }
        let temp = self.root.join(format!(
            ".current.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), ActivationError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(id.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            precommit()?;
            fs::rename(&temp, self.root.join("current"))?;
            File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

#[cfg(test)]
mod interruption_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    #[test]
    fn dangling_current_symlink_is_corrupt_not_uninitialized() {
        use std::os::unix::fs::symlink;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-dangling-pointer-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("snapshots")).unwrap();
        symlink("missing-snapshot-pointer", root.join("current")).unwrap();
        let store = ActiveSnapshotStore::new(&root);
        assert!(matches!(
            store.current(),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn simulated_failure_after_temp_sync_preserves_previous_pointer() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-atomic-precommit-{}-{stamp}",
            std::process::id()
        ));
        for id in ["old", "new"] {
            let path = root.join("snapshots").join(id);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("manifest.json"), "{}").unwrap();
        }
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let error = store.switch_with_precommit_gate("new", || {
            Err(ActivationError::Io(io::Error::other(
                "injected interruption",
            )))
        });
        assert!(matches!(error, Err(ActivationError::Io(_))));
        assert_eq!(store.current().unwrap(), Some("old".into()));
        let leftovers = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".current."))
            .count();
        assert_eq!(leftovers, 0);
        assert!(root.join("snapshots/new/manifest.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
