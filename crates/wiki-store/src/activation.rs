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
    /// The new selector was published but rollback durability is uncertain.
    RollbackFailed {
        commit_error: io::Error,
        rollback_error: io::Error,
    },
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
    Reader(crate::offline_reader::ReaderError),
}

#[derive(Clone, Debug)]
pub struct ActiveSnapshotStore {
    root: PathBuf,
}

pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        // Portable snapshot-directory names must never alias Windows device
        // files, even when this validation runs on Linux or Android.
        && !matches!(id, "con" | "prn" | "aux" | "nul")
        && !(id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && (b'1'..=b'9').contains(&id.as_bytes()[3]))
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

// A selector read must refer to the same stable, singly linked file before
// opening, after reading, and at the pathname. This is defense in depth;
// the snapshot root must still be owned exclusively by the caller.
fn same_pointer_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if !before.file_type().is_file() || !after.file_type().is_file() || before.len() != after.len()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.nlink() == 1
            && after.nlink() == 1
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        before.modified().ok() == after.modified().ok()
            && before.created().ok() == after.created().ok()
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
        self.current_with_after_read(|| {})
    }

    // Injection seam for deterministic replacement during selector reads.
    fn current_with_after_read(
        &self,
        after_read: impl FnOnce(),
    ) -> Result<Option<String>, ActivationError> {
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
        if !same_pointer_snapshot(&meta, &opened_meta) {
            return Err(ActivationError::CorruptCurrentPointer);
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut opened)
            .take(130)
            .read_to_end(&mut bytes)?;
        after_read();
        let after = opened.metadata()?;
        let path_after =
            fs::symlink_metadata(&pointer).map_err(|_| ActivationError::CorruptCurrentPointer)?;
        if bytes.len() > 129
            || bytes.len() as u64 != opened_meta.len()
            || !same_pointer_snapshot(&opened_meta, &after)
            || !same_pointer_snapshot(&after, &path_after)
        {
            return Err(ActivationError::CorruptCurrentPointer);
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

    /// Verify the caller-authenticated immutable catalog, shards and search index
    /// while holding the same media lease as mandatory media verification and
    /// pointer publication. The callback must check the candidate identified by
    /// `id`; this API does not authenticate the upstream publisher.
    pub fn switch_to_verified_with_bound_pack_media(
        &self,
        id: &str,
        store: &MediaObjectStore,
        registry: &MediaRegistry,
        required: &[RequiredMedia],
        verify_reader: impl FnOnce() -> Result<(), crate::offline_reader::ReaderError>,
    ) -> Result<u64, PackMediaActivationError> {
        self.switch_with_bound_pack_media_and_verification(id, store, registry, required, || {
            verify_reader().map_err(PackMediaActivationError::Reader)
        })
    }

    /// Verify the candidate's own on-disk catalog, complete manifest-pinned
    /// article shards, and persisted search index before publishing its pointer.
    /// The layout is snapshots/<id>/{catalog.sqlite,search.sqlite,*shards}.
    /// Verification and required pack-media checks share one media lease.
    ///
    /// The caller MUST authenticate the manifest and the complete shard/media
    /// inventories independently. A matching digest is not publisher trust.
    /// The snapshot tree must remain immutable and exclusively owned by the
    /// installer; this does not defend against non-cooperating filesystem swaps.
    pub fn switch_to_manifest_verified_with_bound_pack_media(
        &self,
        id: &str,
        store: &MediaObjectStore,
        registry: &MediaRegistry,
        required: &[RequiredMedia],
        expected_shards: &std::collections::BTreeMap<String, (u64, String)>,
    ) -> Result<u64, PackMediaActivationError> {
        let candidate = self.root.join("snapshots").join(id);
        self.switch_to_verified_with_bound_pack_media(id, store, registry, required, || {
            let reader =
                crate::offline_reader::VerifiedOfflineCatalogReader::open_with_manifest_shards(
                    &candidate.join("catalog.sqlite"),
                    &candidate.join("search.sqlite"),
                    &candidate,
                    expected_shards,
                )?;
            drop(reader);
            Ok(())
        })
    }

    fn switch_with_bound_pack_media_and_gate(
        &self,
        id: &str,
        store: &MediaObjectStore,
        registry: &MediaRegistry,
        required: &[RequiredMedia],
        after_verification: impl FnOnce(),
    ) -> Result<u64, PackMediaActivationError> {
        self.switch_with_bound_pack_media_and_verification(id, store, registry, required, || {
            after_verification();
            Ok(())
        })
    }

    fn switch_with_bound_pack_media_and_verification(
        &self,
        id: &str,
        store: &MediaObjectStore,
        registry: &MediaRegistry,
        required: &[RequiredMedia],
        verify: impl FnOnce() -> Result<(), PackMediaActivationError>,
    ) -> Result<u64, PackMediaActivationError> {
        if !safe_id(id) {
            return Err(PackMediaActivationError::Activation(
                ActivationError::InvalidSnapshotId,
            ));
        }
        // Reject an unstaged candidate before costly media verification.
        // The final pointer switch repeats this check.
        self.ensure_root()
            .map_err(PackMediaActivationError::Activation)?;
        if !self
            .candidate_is_staged(id)
            .map_err(PackMediaActivationError::Activation)?
        {
            return Err(PackMediaActivationError::Activation(
                ActivationError::CandidateNotStaged,
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
        verify()?;
        self.switch_to_prevalidated(id)
            .map_err(PackMediaActivationError::Activation)?;
        Ok(verified_bytes)
    }

    fn switch_with_precommit_gate(
        &self,
        id: &str,
        precommit: impl FnOnce() -> Result<(), ActivationError>,
    ) -> Result<(), ActivationError> {
        self.switch_with_precommit_and_sync(id, precommit, |root| File::open(root)?.sync_all())
    }

    // Injectable durability boundary for deterministic post-rename failure tests.
    fn switch_with_precommit_and_sync(
        &self,
        id: &str,
        precommit: impl FnOnce() -> Result<(), ActivationError>,
        mut sync_parent: impl FnMut(&Path) -> io::Result<()>,
    ) -> Result<(), ActivationError> {
        if !safe_id(id) {
            return Err(ActivationError::InvalidSnapshotId);
        }
        self.ensure_root()?;
        if !self.candidate_is_staged(id)? {
            return Err(ActivationError::CandidateNotStaged);
        }
        // Never replace a corrupt selector: its previous state cannot be restored.
        let previous = self.current()?;
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
        let mut rollback_temp = None;
        let result = (|| -> Result<(), ActivationError> {
            file.write_all(id.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);

            // Keep a synced copy of the old selector so an unsuccessful
            // directory sync cannot silently leave the new selector active.
            if let Some(previous) = &previous {
                let (path, mut backup) = loop {
                    let candidate = self.root.join(format!(
                        ".previous.{}.{}.tmp",
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
                rollback_temp = Some(path);
                backup.write_all(previous.as_bytes())?;
                backup.write_all(b"\n")?;
                backup.sync_all()?;
                drop(backup);
                sync_parent(&self.root)?;
            }

            precommit()?;
            let current = self.root.join("current");
            fs::rename(&temp, &current)?;
            if let Err(commit_error) = sync_parent(&self.root) {
                let restore = match &rollback_temp {
                    Some(path) => fs::rename(path, &current),
                    None => fs::remove_file(&current),
                };
                let restore = restore.and_then(|()| sync_parent(&self.root));
                return match restore {
                    Ok(()) => Err(ActivationError::Io(commit_error)),
                    Err(rollback_error) => Err(ActivationError::RollbackFailed {
                        commit_error,
                        rollback_error,
                    }),
                };
            }
            Ok(())
        })();
        // An orphan rollback file is never an active selector. Cleanup is
        // best-effort; durability of the active selector is already checked.
        if let Some(path) = rollback_temp {
            let _ = fs::remove_file(path);
        }
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
    fn precommit_rejection_preserves_old_selector_and_cleans_private_files() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-precommit-reject-{}-{stamp}",
            std::process::id()
        ));
        for id in ["old", "new"] {
            let candidate = root.join("snapshots").join(id);
            fs::create_dir_all(&candidate).unwrap();
            fs::write(candidate.join("manifest.json"), b"{}").unwrap();
        }
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let before = fs::read(root.join("current")).unwrap();
        let mut invoked = false;
        let result = store.switch_with_precommit_gate("new", || {
            invoked = true;
            Err(ActivationError::Io(io::Error::other(
                "injected precommit failure",
            )))
        });
        assert!(invoked);
        assert!(matches!(result, Err(ActivationError::Io(_))));
        assert_eq!(fs::read(root.join("current")).unwrap(), before);
        assert_eq!(store.current().unwrap().as_deref(), Some("old"));
        for entry in fs::read_dir(&root).unwrap() {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            assert!(
                !name.starts_with(".current.") && !name.starts_with(".previous."),
                "orphaned private selector file: {name}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_ids_reject_windows_device_names_before_filesystem_access() {
        // The snapshot ID becomes a directory component. Enforce the same
        // portable path policy on Linux and Android, not only on Windows.
        for id in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
            assert!(!safe_id(id), "{id}");
            assert!(matches!(
                ActiveSnapshotStore::new("nonexistent-snapshot-root").switch_to_prevalidated(id),
                Err(ActivationError::InvalidSnapshotId)
            ));
        }
        // Avoid rejecting lookalikes that are ordinary, portable filenames.
        for id in [
            "com0", "com10", "lpt0", "lpt10", "console", "con-old", "aux_2026",
        ] {
            assert!(safe_id(id), "{id}");
        }
    }

    #[test]
    fn manifest_verified_activation_binds_catalog_index_shards_and_media_to_candidate() {
        use crate::catalog::SnapshotCatalog;
        use crate::media_install::install_bound_owned_media;
        use crate::media_ownership::MediaNotice;
        use crate::record_codec::append_record_frame;
        use crate::search_index::OfflineSearchIndex;
        use sha2::{Digest, Sha256};
        use wiki_model::{Article, ArticleKey, PageRecord, Revision, ARTICLE_SCHEMA_VERSION};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-manifest-bound-activation-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        fs::create_dir_all(&media_root).unwrap();
        for id in ["old", "new"] {
            let path = root.join("snapshots").join(id);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("manifest.json"), b"{}").unwrap();
        }
        let snapshots = ActiveSnapshotStore::new(&root);
        snapshots.switch_to_prevalidated("old").unwrap();
        let candidate = root.join("snapshots/new");
        let page = PageRecord::Article(Box::new(Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 1042,
                timestamp: "2026-11-11T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Earth".into(),
            display_title: "Earth".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Earth</article>".into(),
            is_disambiguation: false,
        }));
        let mut shard = Vec::new();
        append_record_frame(&mut shard, &page).unwrap();
        fs::write(candidate.join("articles.shard"), &shard).unwrap();
        let catalog_path = candidate.join("catalog.sqlite");
        let mut catalog = SnapshotCatalog::open(&catalog_path).unwrap();
        catalog
            .insert_verified(&candidate, "articles.shard", 0, shard.len() as u64)
            .unwrap();
        let index_path = candidate.join("search.sqlite");
        let expected_shards = std::collections::BTreeMap::from([(
            "articles.shard".to_owned(),
            (shard.len() as u64, format!("{:x}", Sha256::digest(&shard))),
        )]);
        let store = MediaObjectStore::new(&media_root);
        let mut registry = MediaRegistry::open(&root.join("registry.sqlite")).unwrap();
        registry.bind_media_root(&media_root).unwrap();
        let image = b"required image for candidate";
        let digest = format!("{:x}", Sha256::digest(image));
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Earth.png".into(),
            creator: "Author".into(),
            license: "CC BY-SA 4.0".into(),
        };
        install_bound_owned_media(
            &store,
            &mut registry,
            image,
            &notice,
            OwnerKind::Pack,
            "new",
        )
        .unwrap();
        let required = [RequiredMedia {
            digest,
            bytes: image.len() as u64,
        }];

        // No actual derived index: a valid manifest and media cannot activate.
        let attempt = snapshots.switch_to_manifest_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &required,
            &expected_shards,
        );
        assert!(matches!(attempt, Err(PackMediaActivationError::Reader(_))));
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));

        OfflineSearchIndex::replace_persisted_from_catalog(&index_path, &catalog, &candidate)
            .unwrap();
        let mut wrong_shards = expected_shards.clone();
        wrong_shards.get_mut("articles.shard").unwrap().1 = "0".repeat(64);
        let attempt = snapshots.switch_to_manifest_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &required,
            &wrong_shards,
        );
        assert!(matches!(attempt, Err(PackMediaActivationError::Reader(_))));
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));

        // An altered immutable shard must also preserve the previous pointer.
        fs::write(candidate.join("articles.shard"), b"corrupt shard").unwrap();
        let attempt = snapshots.switch_to_manifest_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &required,
            &expected_shards,
        );
        assert!(matches!(attempt, Err(PackMediaActivationError::Reader(_))));
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));
        fs::write(candidate.join("articles.shard"), &shard).unwrap();

        let verified = snapshots
            .switch_to_manifest_verified_with_bound_pack_media(
                "new",
                &store,
                &registry,
                &required,
                &expected_shards,
            )
            .unwrap();
        assert_eq!(verified, image.len() as u64);
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("new"));
        drop(catalog);
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_activation_preserves_old_pointer_on_reader_failure() {
        use crate::media_install::install_bound_owned_media;
        use crate::media_ownership::MediaNotice;
        use sha2::{Digest, Sha256};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-activation-reader-gate-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        fs::create_dir_all(&media_root).unwrap();
        for id in ["old", "new"] {
            let candidate = root.join("snapshots").join(id);
            fs::create_dir_all(&candidate).unwrap();
            fs::write(candidate.join("manifest.json"), b"{}").unwrap();
        }
        let snapshots = ActiveSnapshotStore::new(&root);
        snapshots.switch_to_prevalidated("old").unwrap();
        let store = MediaObjectStore::new(&media_root);
        let mut registry = MediaRegistry::open(&root.join("registry.sqlite")).unwrap();
        registry.bind_media_root(&media_root).unwrap();
        let bytes = b"mandatory candidate image";
        let digest = format!("{:x}", Sha256::digest(bytes));
        let required = [RequiredMedia {
            digest: digest.clone(),
            bytes: bytes.len() as u64,
        }];
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Candidate.png".into(),
            creator: "Author".into(),
            license: "CC BY-SA 4.0".into(),
        };
        install_bound_owned_media(
            &store,
            &mut registry,
            bytes,
            &notice,
            OwnerKind::Pack,
            "new",
        )
        .unwrap();

        let mut invoked = false;
        let result = snapshots.switch_to_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &required,
            || {
                invoked = true;
                Err(crate::offline_reader::ReaderError::StaleRevision {
                    expected: 1,
                    actual: 2,
                })
            },
        );
        assert!(invoked);
        assert!(matches!(result, Err(PackMediaActivationError::Reader(_))));
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));

        let object = media_root.join(&digest[..2]).join(&digest);
        fs::write(&object, b"corrupt").unwrap();
        invoked = false;
        let result = snapshots.switch_to_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &required,
            || {
                invoked = true;
                Ok(())
            },
        );
        assert!(matches!(result, Err(PackMediaActivationError::Media(_))));
        assert!(
            !invoked,
            "reader must not run after mandatory media failure"
        );
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));
        fs::write(&object, bytes).unwrap();

        let verified = snapshots
            .switch_to_verified_with_bound_pack_media(
                "new",
                &store,
                &registry,
                &required,
                || Ok(()),
            )
            .unwrap();
        assert_eq!(verified, bytes.len() as u64);
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("new"));
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_activation_rejects_unstaged_candidate_without_reader_callback() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-activation-unstaged-gate-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        fs::create_dir_all(&media_root).unwrap();
        let old = root.join("snapshots/old");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("manifest.json"), b"{}").unwrap();
        let snapshots = ActiveSnapshotStore::new(&root);
        snapshots.switch_to_prevalidated("old").unwrap();

        let store = MediaObjectStore::new(&media_root);
        let mut registry = MediaRegistry::open(&root.join("registry.sqlite")).unwrap();
        registry.bind_media_root(&media_root).unwrap();

        // A directory without a manifest is not staged, even when it exists.
        fs::create_dir_all(root.join("snapshots/new")).unwrap();
        let mut invoked = false;
        let result = snapshots.switch_to_verified_with_bound_pack_media(
            "new",
            &store,
            &registry,
            &[],
            || {
                invoked = true;
                Ok(())
            },
        );
        assert!(matches!(
            result,
            Err(PackMediaActivationError::Activation(
                ActivationError::CandidateNotStaged
            ))
        ));
        assert!(
            !invoked,
            "unstaged candidate must not invoke reader verification"
        );
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));

        // The same rule applies to a completely missing candidate.
        let result = snapshots.switch_to_verified_with_bound_pack_media(
            "missing",
            &store,
            &registry,
            &[],
            || {
                invoked = true;
                Ok(())
            },
        );
        assert!(matches!(
            result,
            Err(PackMediaActivationError::Activation(
                ActivationError::CandidateNotStaged
            ))
        ));
        assert!(!invoked);
        assert_eq!(snapshots.current().unwrap().as_deref(), Some("old"));

        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

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
        assert!(matches!(
            attempt,
            Err(PackMediaActivationError::Registry(_))
        ));
        registry.bind_media_root(&media_root).unwrap();
        let attempt = snapshot_store.switch_to_prevalidated_with_bound_pack_media(
            "new",
            &wrong_store,
            &registry,
            &required,
        );
        assert!(matches!(
            attempt,
            Err(PackMediaActivationError::Registry(_))
        ));
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
    fn current_rejects_same_bytes_path_replacement_after_read() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-selector-replaced-{}-{stamp}",
            std::process::id()
        ));
        let snapshot = root.join("snapshots/old");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("manifest.json"), "{}").unwrap();
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let current = root.join("current");
        let retired = root.join("retired");
        assert!(matches!(
            store.current_with_after_read(|| {
                fs::rename(&current, &retired).unwrap();
                fs::write(&current, b"old\n").unwrap();
            }),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        // The replacement has the same length and digest but a different inode.
        assert_eq!(fs::read(&current).unwrap(), fs::read(&retired).unwrap());
        assert_eq!(store.current().unwrap(), Some("old".to_owned()));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn current_rejects_symlink_substitution_after_read() {
        use std::os::unix::fs::symlink;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-selector-symlink-swap-{}-{stamp}",
            std::process::id()
        ));
        let snapshot = root.join("snapshots/old");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("manifest.json"), "{}").unwrap();
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let current = root.join("current");
        let retired = root.join("retired");
        assert!(matches!(
            store.current_with_after_read(|| {
                fs::rename(&current, &retired).unwrap();
                symlink("retired", &current).unwrap();
            }),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        fs::remove_file(&current).unwrap();
        fs::rename(&retired, &current).unwrap();
        assert_eq!(store.current().unwrap(), Some("old".to_owned()));
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

    #[test]
    fn post_rename_sync_failure_restores_old_or_absent_selector() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        for had_previous in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "wiki-selector-rollback-{}-{stamp}-{had_previous}",
                std::process::id()
            ));
            for id in ["old", "new"] {
                let path = root.join("snapshots").join(id);
                fs::create_dir_all(&path).unwrap();
                fs::write(path.join("manifest.json"), "{}").unwrap();
            }
            let store = ActiveSnapshotStore::new(&root);
            if had_previous {
                store.switch_to_prevalidated("old").unwrap();
            }
            let mut calls = 0;
            let fail_at = if had_previous { 2 } else { 1 };
            let result = store.switch_with_precommit_and_sync(
                "new",
                || Ok(()),
                |path| {
                    calls += 1;
                    if calls == fail_at {
                        Err(io::Error::other("injected post-rename fsync failure"))
                    } else {
                        File::open(path)?.sync_all()
                    }
                },
            );
            assert!(matches!(result, Err(ActivationError::Io(_))));
            assert_eq!(calls, fail_at + 1);
            assert_eq!(
                store.current().unwrap(),
                had_previous.then(|| "old".to_owned())
            );
            assert_eq!(root.join("current").exists(), had_previous);
            assert_eq!(
                fs::read_dir(&root)
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        let name = entry.file_name();
                        let name = name.to_string_lossy();
                        name.starts_with(".current.") || name.starts_with(".previous.")
                    })
                    .count(),
                0
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn rollback_backup_sync_failure_never_publishes_candidate() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-selector-backup-sync-{}-{stamp}",
            std::process::id()
        ));
        for id in ["old", "new"] {
            let path = root.join("snapshots").join(id);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("manifest.json"), "{}").unwrap();
        }
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let original = fs::read(root.join("current")).unwrap();
        let mut sync_calls = 0;
        let mut precommit_called = false;
        let result = store.switch_with_precommit_and_sync(
            "new",
            || {
                precommit_called = true;
                Ok(())
            },
            |_| {
                sync_calls += 1;
                Err(io::Error::other("injected backup directory sync failure"))
            },
        );
        assert!(matches!(result, Err(ActivationError::Io(_))));
        assert_eq!(sync_calls, 1);
        assert!(!precommit_called, "candidate must not reach commit gate");
        assert_eq!(fs::read(root.join("current")).unwrap(), original);
        assert_eq!(store.current().unwrap().as_deref(), Some("old"));
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with(".current.") || name.starts_with(".previous.")
                })
                .count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_rollback_sync_reports_uncertain_durability() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-selector-uncertain-{}-{stamp}",
            std::process::id()
        ));
        for id in ["old", "new"] {
            let path = root.join("snapshots").join(id);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("manifest.json"), "{}").unwrap();
        }
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();
        let mut calls = 0;
        let result = store.switch_with_precommit_and_sync(
            "new",
            || Ok(()),
            |path| {
                calls += 1;
                if calls >= 2 {
                    Err(io::Error::other("injected directory durability failure"))
                } else {
                    File::open(path)?.sync_all()
                }
            },
        );
        assert!(matches!(
            result,
            Err(ActivationError::RollbackFailed { .. })
        ));
        assert_eq!(calls, 3);
        // Logical rollback succeeded, but power-loss survival is not proven.
        assert_eq!(store.current().unwrap(), Some("old".into()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_selector_is_not_overwritten_without_explicit_recovery() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-selector-corrupt-{}-{stamp}",
            std::process::id()
        ));
        let path = root.join("snapshots/new");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("manifest.json"), "{}").unwrap();
        fs::write(root.join("current"), b"../unsafe\n").unwrap();
        let store = ActiveSnapshotStore::new(&root);
        assert!(matches!(
            store.switch_to_prevalidated("new"),
            Err(ActivationError::CorruptCurrentPointer)
        ));
        assert_eq!(fs::read(root.join("current")).unwrap(), b"../unsafe\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inactive_pack_release_rejects_non_snapshot_owner_ids_without_revocation() {
        use crate::media_gc::{
            release_inactive_snapshot_pack_and_reclaim_media, InactivePackReleaseError,
        };
        use crate::media_install::install_bound_owned_media;
        use crate::media_ownership::MediaNotice;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-invalid-snapshot-release-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        fs::create_dir_all(&media_root).unwrap();
        let candidate = root.join("snapshots/active");
        fs::create_dir_all(&candidate).unwrap();
        fs::write(candidate.join("manifest.json"), b"{}").unwrap();
        let snapshots = ActiveSnapshotStore::new(&root);
        snapshots.switch_to_prevalidated("active").unwrap();
        let store = MediaObjectStore::new(&media_root);
        let mut registry = MediaRegistry::open(&root.join("ownership.sqlite")).unwrap();
        registry.bind_media_root(&media_root).unwrap();
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Diagram.png".into(),
            creator: "Author".into(),
            license: "CC BY-SA 4.0".into(),
        };

        // The registry intentionally accepts a broader owner-ID grammar than
        // snapshot activation. A pack owner that cannot name a snapshot must
        // not be released through the snapshot-specific deletion boundary.
        for id in [
            "Bad".to_owned(),
            "1bad".to_owned(),
            "_bad".to_owned(),
            "con".to_owned(),
            "com1".to_owned(),
            "lpt9".to_owned(),
            "x".repeat(129),
        ] {
            let bytes = format!("required diagram for {id}");
            let digest = install_bound_owned_media(
                &store,
                &mut registry,
                bytes.as_bytes(),
                &notice,
                OwnerKind::Pack,
                &id,
            )
            .unwrap();
            assert!(matches!(
                release_inactive_snapshot_pack_and_reclaim_media(
                    &snapshots,
                    &store,
                    &mut registry,
                    &id,
                    10,
                ),
                Err(InactivePackReleaseError::Activation(
                    ActivationError::InvalidSnapshotId
                ))
            ));
            assert_eq!(
                registry
                    .owned_digest_size(OwnerKind::Pack, &id, &digest)
                    .unwrap(),
                Some(bytes.len() as u64)
            );
            assert_eq!(store.read_verified(&digest).unwrap(), bytes.as_bytes());
        }
        assert_eq!(snapshots.current().unwrap(), Some("active".into()));
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inactive_pack_release_fails_closed_for_corrupt_active_pointer_or_manifest() {
        use crate::media_gc::{
            release_inactive_snapshot_pack_and_reclaim_media, InactivePackReleaseError,
        };
        use crate::media_install::install_bound_owned_media;
        use crate::media_ownership::MediaNotice;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-corrupt-active-release-{}-{stamp}",
            std::process::id()
        ));
        let media_root = root.join("media");
        fs::create_dir_all(&media_root).unwrap();
        for id in ["old", "new"] {
            let candidate = root.join("snapshots").join(id);
            fs::create_dir_all(&candidate).unwrap();
            fs::write(candidate.join("manifest.json"), b"{}").unwrap();
        }
        let snapshots = ActiveSnapshotStore::new(&root);
        let store = MediaObjectStore::new(&media_root);
        let mut registry = MediaRegistry::open(&root.join("media-registry.sqlite")).unwrap();
        registry.bind_media_root(&media_root).unwrap();
        let bytes = b"active snapshot required diagram";
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Diagram.png".into(),
            creator: "Author".into(),
            license: "CC BY-SA 4.0".into(),
        };
        let digest = install_bound_owned_media(
            &store,
            &mut registry,
            bytes,
            &notice,
            OwnerKind::Pack,
            "old",
        )
        .unwrap();
        let required = [RequiredMedia {
            digest: digest.clone(),
            bytes: bytes.len() as u64,
        }];
        snapshots
            .switch_to_prevalidated_with_bound_pack_media("old", &store, &registry, &required)
            .unwrap();

        // An unreadable active pointer must never be interpreted as "no
        // active snapshot" and used to revoke the only copy of its media.
        fs::write(root.join("current"), b"invalid active pointer\n").unwrap();
        let result = release_inactive_snapshot_pack_and_reclaim_media(
            &snapshots,
            &store,
            &mut registry,
            "old",
            10,
        );
        assert!(matches!(
            result,
            Err(InactivePackReleaseError::Activation(
                ActivationError::CorruptCurrentPointer
            ))
        ));
        assert_eq!(
            registry.owned_media(OwnerKind::Pack, "old").unwrap().len(),
            1
        );
        assert_eq!(store.read_verified(&digest).unwrap(), bytes);

        // A well-formed pointer to a snapshot with missing manifest is
        // equally unsafe: it must not become an implicit deletion permit.
        fs::write(root.join("current"), b"old\n").unwrap();
        fs::remove_file(root.join("snapshots/old/manifest.json")).unwrap();
        let result = release_inactive_snapshot_pack_and_reclaim_media(
            &snapshots,
            &store,
            &mut registry,
            "old",
            10,
        );
        assert!(matches!(
            result,
            Err(InactivePackReleaseError::Activation(
                ActivationError::CorruptCurrentPointer
            ))
        ));
        assert_eq!(
            registry.owned_media(OwnerKind::Pack, "old").unwrap().len(),
            1
        );
        assert_eq!(store.read_verified(&digest).unwrap(), bytes);

        fs::write(root.join("snapshots/old/manifest.json"), b"{}").unwrap();
        assert_eq!(snapshots.current().unwrap(), Some("old".into()));
        assert!(matches!(
            release_inactive_snapshot_pack_and_reclaim_media(
                &snapshots,
                &store,
                &mut registry,
                "old",
                10
            ),
            Err(InactivePackReleaseError::ActiveSnapshot)
        ));
        assert_eq!(
            registry.owned_media(OwnerKind::Pack, "old").unwrap().len(),
            1
        );
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }
}
