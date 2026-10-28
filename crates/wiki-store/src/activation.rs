//! Crash-conscious active snapshot pointer; partial STORE-003.
//!
//! This is only the atomic SWITCH primitive. Callers must independently
//! validate every snapshot byte, manifest, and schema before invoking it.
//! An existing manifest alone does not establish upstream authenticity.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::media_install::{verify_required_owner_media_unlocked, MediaInstallError};
use crate::media_inventory::RequiredMedia;
use crate::media_lease::{MediaLeaseCoordinator, MediaLeaseError};
use crate::media_objects::MediaObjectStore;
use crate::media_ownership::{MediaRegistry, MediaRegistryError, OwnerKind};

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

/// An opt-in composition gate for already-authenticated, prevalidated packs.
/// This cannot authenticate a manifest or substitute for catalog verification.
#[derive(Debug)]
pub enum PackMediaActivationError {
    Activation(ActivationError),
    Lease(MediaLeaseError),
    Registry(MediaRegistryError),
    Media(MediaInstallError),
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
        // Validate each existing path component, not merely the final root.
        // The caller still must own these directories exclusively: this
        // preflight cannot prevent concurrent hostile filesystem swaps.
        for ancestor in self.root.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
                return Err(ActivationError::UnsafeRoot);
            }
        }
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
        let meta = match fs::symlink_metadata(&pointer) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ActivationError::Io(error)),
        };
        // A valid pointer consists of a snapshot ID (at most 128 ASCII
        // bytes) and an optional trailing newline. Refuse unbounded reads,
        // special files and dangling or live symlinks alike.
        if !meta.file_type().is_file() || meta.len() > 129 {
            return Err(ActivationError::CorruptCurrentPointer);
        }
        // Read through the descriptor we inspected, rather than reopening
        // the path by name during a possible concurrent replacement.
        let mut opened = File::open(&pointer)?;
        let opened_meta = opened.metadata()?;
        if !opened_meta.is_file() || opened_meta.len() != meta.len() {
            return Err(ActivationError::CorruptCurrentPointer);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // An outside hard link could rewrite the active snapshot selector
            // without going through the verified atomic switch protocol.
            if meta.ino() != opened_meta.ino()
                || meta.dev() != opened_meta.dev()
                || meta.nlink() != 1
                || opened_meta.nlink() != 1
            {
                return Err(ActivationError::CorruptCurrentPointer);
            }
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut opened)
            .take(130)
            .read_to_end(&mut bytes)?;
        let after = opened.metadata()?;
        if bytes.len() > 129
            || bytes.len() as u64 != opened_meta.len()
            || after.len() != opened_meta.len()
        {
            return Err(ActivationError::CorruptCurrentPointer);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if after.nlink() != 1 {
                return Err(ActivationError::CorruptCurrentPointer);
            }
        }
        let id = std::str::from_utf8(&bytes).map_err(|_| ActivationError::CorruptCurrentPointer)?;
        let id = id.strip_suffix('\n').unwrap_or(id);
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

    /// Activate a PREVALIDATED snapshot only if its pack owner has exactly
    /// the caller-authenticated required-media inventory, all object bytes
    /// rehash correctly, and the registry is bound to the supplied media root.
    ///
    /// The snapshot ID is also the pack owner ID: this prevents accidentally
    /// authorizing a different snapshot with another pack's owner claims.
    /// One SHARED media lease covers root binding, owner/inventory verification,
    /// and the durable pointer switch. A cooperating exclusive collector cannot
    /// reclaim the verified media between the check and activation.
    ///
    /// This is NOT an authenticated pack installer: callers must independently
    /// authenticate the manifest and validate article/catalog/shard provenance
    /// before calling; active media owners must be retained until deactivation.
    pub fn switch_to_prevalidated_with_bound_pack_media(
        &self,
        id: &str,
        store: &MediaObjectStore,
        registry: &MediaRegistry,
        required: &[RequiredMedia],
    ) -> Result<u64, PackMediaActivationError> {
        self.switch_with_bound_pack_media_and_gate(id, store, registry, required, || {})
    }

    fn switch_with_bound_pack_media_and_gate(
        &self,
        id: &str,
        store: &MediaObjectStore,
        registry: &MediaRegistry,
        required: &[RequiredMedia],
        after_verification: impl FnOnce(),
    ) -> Result<u64, PackMediaActivationError> {
        if !safe_id(id) {
            return Err(PackMediaActivationError::Activation(
                ActivationError::InvalidSnapshotId,
            ));
        }
        let _lease = MediaLeaseCoordinator::new(store.root())
            .acquire_shared()
            .map_err(PackMediaActivationError::Lease)?;
        registry
            .require_media_root_binding(store.root())
            .map_err(PackMediaActivationError::Registry)?;
        let verified_bytes =
            verify_required_owner_media_unlocked(store, registry, OwnerKind::Pack, id, required)
                .map_err(PackMediaActivationError::Media)?;
        after_verification();
        self.switch_to_prevalidated(id)
            .map_err(PackMediaActivationError::Activation)?;
        Ok(verified_bytes)
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
        // A previous crash may leave a private temporary path behind.
        // Never truncate or adopt it: pick a fresh create_new name instead.
        let (temp, mut file) = loop {
            let candidate = self.root.join(format!(
                ".current.{}.{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(ActivationError::Io(error)),
            }
        };
        let result = (|| -> Result<(), ActivationError> {
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

    #[test]
    fn bound_pack_activation_refuses_missing_wrong_root_and_corrupt_media() {
        use crate::media_install::install_bound_owned_media;
        use crate::media_ownership::MediaNotice;
        use sha2::{Digest, Sha256};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-activation-bound-media-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        let other_root = root.join("wrong-media");
        fs::create_dir_all(&media_root).unwrap();
        fs::create_dir_all(&other_root).unwrap();
        for id in ["old", "new"] {
            let snapshot = root.join("snapshots").join(id);
            fs::create_dir_all(&snapshot).unwrap();
            fs::write(snapshot.join("manifest.json"), b"{}").unwrap();
        }
        let snapshot_store = ActiveSnapshotStore::new(&root);
        snapshot_store.switch_to_prevalidated("old").unwrap();
        let store = MediaObjectStore::new(&media_root);
        let wrong_store = MediaObjectStore::new(&other_root);
        let db = root.join("ownership.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        let bytes = b"required image for new snapshot";
        let digest = format!("{:x}", Sha256::digest(bytes));
        let required = [RequiredMedia {
            digest: digest.clone(),
            bytes: bytes.len() as u64,
        }];
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Photo.png".into(),
            creator: "Author".into(),
            license: "CC BY-SA 4.0".into(),
        };

        // Unbound and wrong-root registries cannot authorize a pointer switch.
        let attempt = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("new", &store, &registry, &required);
        assert!(matches!(attempt, Err(PackMediaActivationError::Registry(_))));
        registry.bind_media_root(&media_root).unwrap();
        let attempt = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("new", &wrong_store, &registry, &required);
        assert!(matches!(attempt, Err(PackMediaActivationError::Registry(_))));
        // A valid registry binding is not enough without exact owner claims.
        let attempt = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("new", &store, &registry, &required);
        assert!(matches!(attempt, Err(PackMediaActivationError::Media(_))));
        assert_eq!(snapshot_store.current().unwrap(), Some("old".into()));

        install_bound_owned_media(
            &store,
            &mut registry,
            bytes,
            &notice,
            OwnerKind::Pack,
            "new",
        )
        .unwrap();
        let incorrect = [RequiredMedia {
            digest: digest.clone(),
            bytes: (bytes.len() + 1) as u64,
        }];
        let attempt = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("new", &store, &registry, &incorrect);
        assert!(matches!(attempt, Err(PackMediaActivationError::Media(_))));
        let object = media_root.join(&digest[..2]).join(&digest);
        fs::write(&object, b"corrupted image").unwrap();
        let attempt = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("new", &store, &registry, &required);
        assert!(matches!(attempt, Err(PackMediaActivationError::Media(_))));
        assert_eq!(snapshot_store.current().unwrap(), Some("old".into()));
        fs::write(&object, bytes).unwrap();

        let activated_bytes = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("new", &store, &registry, &required)
            .unwrap();
        assert_eq!(activated_bytes, bytes.len() as u64);
        assert_eq!(snapshot_store.current().unwrap(), Some("new".into()));
        // A different snapshot cannot borrow this pack's registered media.
        let attempt = snapshot_store
            .switch_to_prevalidated_with_bound_pack_media("old", &store, &registry, &required);
        assert!(matches!(attempt, Err(PackMediaActivationError::Media(_))));
        assert_eq!(snapshot_store.current().unwrap(), Some("new".into()));
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pack_media_lease_covers_verification_and_pointer_switch() {
        use crate::media_install::install_bound_owned_media;
        use crate::media_ownership::MediaNotice;
        use rusqlite::Connection;
        use sha2::{Digest, Sha256};
        use std::time::Duration;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-activation-media-lease-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        let candidate = root.join("snapshots/new");
        fs::create_dir_all(&media_root).unwrap();
        fs::create_dir_all(&candidate).unwrap();
        fs::write(candidate.join("manifest.json"), b"{}").unwrap();
        let snapshot_store = ActiveSnapshotStore::new(&root);
        let store = MediaObjectStore::new(&media_root);
        let db = root.join("ownership.sqlite");
        let mut registry = MediaRegistry::open(&db).unwrap();
        registry.bind_media_root(&media_root).unwrap();
        let bytes = b"protected visual";
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Protected.png".into(),
            creator: "Author".into(),
            license: "CC BY-SA 4.0".into(),
        };
        let digest = install_bound_owned_media(
            &store,
            &mut registry,
            bytes,
            &notice,
            OwnerKind::Pack,
            "new",
        )
        .unwrap();
        let required = [RequiredMedia {
            digest: digest.clone(),
            bytes: bytes.len() as u64,
        }];
        assert_eq!(digest, format!("{:x}", Sha256::digest(bytes)));
        snapshot_store
            .switch_with_bound_pack_media_and_gate("new", &store, &registry, &required, || {
                let contender = Connection::open(media_root.join(".media-lease.sqlite")).unwrap();
                contender.busy_timeout(Duration::from_millis(0)).unwrap();
                assert!(contender.execute_batch("BEGIN EXCLUSIVE;").is_err());
            })
            .unwrap();
        let contender = Connection::open(media_root.join(".media-lease.sqlite")).unwrap();
        contender.busy_timeout(Duration::from_millis(0)).unwrap();
        contender
            .execute_batch("BEGIN EXCLUSIVE; ROLLBACK;")
            .unwrap();
        assert_eq!(snapshot_store.current().unwrap(), Some("new".into()));
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_root_with_symlinked_parent_is_not_accepted() {
        use std::os::unix::fs::symlink;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "wiki-snapshot-ancestor-{}-{stamp}",
            std::process::id()
        ));
        let root = base.join("real/data");
        let candidate = root.join("snapshots/old");
        fs::create_dir_all(&candidate).unwrap();
        fs::write(candidate.join("manifest.json"), "{}").unwrap();
        let canonical = ActiveSnapshotStore::new(&root);
        canonical.switch_to_prevalidated("old").unwrap();
        assert_eq!(canonical.current().unwrap(), Some("old".into()));

        symlink(base.join("real"), base.join("alias")).unwrap();
        let via_alias = ActiveSnapshotStore::new(base.join("alias/data"));
        assert!(matches!(
            via_alias.current(),
            Err(ActivationError::UnsafeRoot)
        ));
        assert!(matches!(
            via_alias.switch_to_prevalidated("old"),
            Err(ActivationError::UnsafeRoot)
        ));
        assert_eq!(canonical.current().unwrap(), Some("old".into()));

        fs::remove_dir_all(base).unwrap();
    }

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

    #[cfg(unix)]
    #[test]
    fn externally_hardlinked_current_pointer_is_not_trusted() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-hardlink-pointer-{}-{stamp}",
            std::process::id()
        ));
        let snapshot = root.join("snapshots/old");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("manifest.json"), "{}").unwrap();
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let outside = root.with_extension("external-current");
        fs::hard_link(root.join("current"), &outside).unwrap();
        assert!(matches!(
            store.current(),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        fs::remove_file(outside).unwrap();
        assert_eq!(store.current().unwrap(), Some("old".into()));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn live_symlink_to_valid_snapshot_pointer_is_never_accepted() {
        use std::os::unix::fs::symlink;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-live-link-{}-{stamp}", std::process::id()));
        let snapshot = root.join("snapshots/old");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("manifest.json"), "{}").unwrap();
        fs::write(root.join("saved-current"), b"old\n").unwrap();
        symlink("saved-current", root.join("current")).unwrap();
        let store = ActiveSnapshotStore::new(&root);
        assert!(matches!(
            store.current(),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_and_invalid_utf8_pointer_are_rejected_before_snapshot_lookup() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-invalid-pointer-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("snapshots")).unwrap();
        let pointer = root.join("current");
        let store = ActiveSnapshotStore::new(&root);
        assert_eq!(store.current().unwrap(), None);

        fs::write(&pointer, "x".repeat(130)).unwrap();
        assert!(matches!(
            store.current(),
            Err(ActivationError::CorruptCurrentPointer)
        ));

        fs::write(&pointer, b"\xff\xfe").unwrap();
        assert!(matches!(
            store.current(),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abandoned_current_temporaries_do_not_block_snapshot_activation() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-abandoned-pointer-{}-{stamp}",
            std::process::id()
        ));
        let candidate = root.join("snapshots/new");
        fs::create_dir_all(&candidate).unwrap();
        fs::write(candidate.join("manifest.json"), "{}").unwrap();
        let store = ActiveSnapshotStore::new(&root);
        let next = NEXT_TEMP.load(Ordering::Relaxed);
        let abandoned: Vec<_> = (next..next + 32)
            .map(|index| root.join(format!(".current.{}.{}.tmp", std::process::id(), index)))
            .collect();
        for path in &abandoned {
            fs::write(path, b"incomplete previous activation").unwrap();
        }
        store.switch_to_prevalidated("new").unwrap();
        assert_eq!(store.current().unwrap(), Some("new".into()));
        for path in &abandoned {
            assert_eq!(fs::read(path).unwrap(), b"incomplete previous activation");
        }
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
