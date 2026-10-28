//! Install media bytes before recording durable ownership.
//!
//! This is a building block, not an offline-ready pack transaction: callers
//! must verify the authenticated manifest and every required visual before
//! activating a snapshot. An interrupted installation may leave unowned
//! content-addressed bytes; it must never make missing bytes look installed.

use crate::media_inventory::{verify_declared_media_unlocked, MediaInventoryError, RequiredMedia};
use crate::media_lease::{MediaLeaseCoordinator, MediaLeaseError};
use crate::media_objects::{MediaObjectError, MediaObjectStore};
use crate::media_ownership::{
    valid_owner_id, MediaNotice, MediaRegistry, MediaRegistryError, OwnerKind,
};

#[derive(Debug)]
pub enum MediaInstallError {
    Store(MediaObjectError),
    Lease(MediaLeaseError),
    Registry(MediaRegistryError),
    IdentityMismatch,
    Inventory(MediaInventoryError),
}

/// Verified bytes and the complete, validated attribution notices for one
/// digest claimed by the requested pack, cache entry, or user pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedOwnedMedia {
    pub bytes: Vec<u8>,
    pub notices: Vec<MediaNotice>,
}

/// Read an owner's media bytes and attribution under a single shared
/// cross-process media lease. A future collector holding the exclusive lease
/// cannot remove the underlying file between ownership lookup and the
/// completed SHA-256-verified read.
///
/// This checks *local* ownership and notice integrity, not publisher trust,
/// authenticated pack activation, or access authorization. Other registry
/// connections can revoke ownership concurrently; the lease protects file
/// lifetime, not an immutable ownership snapshot.
pub fn read_verified_owned_media(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    digest: &str,
) -> Result<Option<VerifiedOwnedMedia>, MediaInstallError> {
    read_verified_owned_media_with_binding(store, registry, kind, owner_id, digest, false)
}

/// Production owner-scoped media read: reject an unbound or wrong-root
/// registry before consulting its owner claims, notices or object bytes.
/// The binding check and SHA-256 read share one SHARED media lease.
pub fn read_verified_bound_owned_media(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    digest: &str,
) -> Result<Option<VerifiedOwnedMedia>, MediaInstallError> {
    read_verified_owned_media_with_binding(store, registry, kind, owner_id, digest, true)
}

fn read_verified_owned_media_with_binding(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    digest: &str,
    require_binding: bool,
) -> Result<Option<VerifiedOwnedMedia>, MediaInstallError> {
    // Acquire before checking binding or ownership: a collector must not
    // slip between the lookup and file open. Nested SQLite leases deadlock.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaInstallError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaInstallError::Registry)?;
    }
    let Some(declared_bytes) = registry
        .owned_digest_size(kind, owner_id, digest)
        .map_err(MediaInstallError::Registry)?
    else {
        return Ok(None);
    };
    let notices = registry
        .owned_digest_notices(kind, owner_id, digest)
        .map_err(MediaInstallError::Registry)?;
    let bytes = store
        .read_verified_unlocked(digest)
        .map_err(MediaInstallError::Store)?;
    if bytes.len() as u64 != declared_bytes {
        return Err(MediaInstallError::IdentityMismatch);
    }
    Ok(Some(VerifiedOwnedMedia { bytes, notices }))
}

/// Persist and rehash the object first, then register attribution and owner.
/// A failure before the final ownership write must not activate a pack.
pub fn install_owned_media(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    content: &[u8],
    notice: &MediaNotice,
    kind: OwnerKind,
    owner_id: &str,
) -> Result<String, MediaInstallError> {
    install_owned_media_with_binding(store, registry, content, notice, kind, owner_id, false)
}

/// Production entry point for a root-bound owner installation. The SHARED
/// cross-process media lease covers the binding check, byte publication and
/// SQLite owner registration. A wrong or unbound registry fails *before*
/// publishing any bytes. This does not authenticate upstream media or activate
/// a pack; callers must independently verify the signed required-media list.
///
/// The unbound installer remains available for controlled import fixtures and
/// initial migration. Production callers using root-bound GC must use this
/// bound entry point rather than bypassing the registry-to-root association.
pub fn install_bound_owned_media(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    content: &[u8],
    notice: &MediaNotice,
    kind: OwnerKind,
    owner_id: &str,
) -> Result<String, MediaInstallError> {
    install_owned_media_with_binding(store, registry, content, notice, kind, owner_id, true)
}

fn install_owned_media_with_binding(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    content: &[u8],
    notice: &MediaNotice,
    kind: OwnerKind,
    owner_id: &str,
    require_binding: bool,
) -> Result<String, MediaInstallError> {
    if !valid_owner_id(owner_id) || !notice.validate() {
        return Err(MediaInstallError::Registry(
            MediaRegistryError::InvalidInput,
        ));
    }
    // Hold the shared lease from binding validation through publication and
    // the final owner commit. Never acquire a nested lease in this scope.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaInstallError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaInstallError::Registry)?;
    }
    let stored_digest = store
        .store_bytes_unlocked(content)
        .map_err(MediaInstallError::Store)?;
    // Keep object identity, attribution and ownership in one SQLite commit.
    // Published bytes may remain orphaned after failure, but registry metadata
    // must not be partially committed.
    let registered_digest = registry
        .register_verified_owned_single(content, notice, kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    if stored_digest != registered_digest {
        return Err(MediaInstallError::IdentityMismatch);
    }
    Ok(stored_digest)
}

/// Stage every object before atomically registering attribution and ownership
/// for the complete batch. A failed stage may leave unowned deduplicated bytes,
/// but cannot leave a partially registered pack. This is not pack activation:
/// the caller must authenticate the manifest and verify required visuals.
pub fn install_owned_media_batch(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    assets: &[(&[u8], &MediaNotice)],
    kind: OwnerKind,
    owner_id: &str,
) -> Result<Vec<String>, MediaInstallError> {
    install_owned_media_batch_with_binding(store, registry, assets, kind, owner_id, false)
}

/// Install an entire owner-scoped batch only into the registry's bound media
/// root. The SHARED lease spans the root check, every staged object, and the
/// atomic batch ownership transaction. An unbound or mismatched registry is
/// rejected before creating even the first published digest.
pub fn install_bound_owned_media_batch(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    assets: &[(&[u8], &MediaNotice)],
    kind: OwnerKind,
    owner_id: &str,
) -> Result<Vec<String>, MediaInstallError> {
    install_owned_media_batch_with_binding(store, registry, assets, kind, owner_id, true)
}

fn install_owned_media_batch_with_binding(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    assets: &[(&[u8], &MediaNotice)],
    kind: OwnerKind,
    owner_id: &str,
    require_binding: bool,
) -> Result<Vec<String>, MediaInstallError> {
    if !valid_owner_id(owner_id)
        || assets.is_empty()
        || assets.len() > 100_000
        || assets.iter().any(|(bytes, notice)| {
            bytes.is_empty() || bytes.len() > 64 * 1024 * 1024 || !notice.validate()
        })
    {
        return Err(MediaInstallError::Registry(
            MediaRegistryError::InvalidInput,
        ));
    }
    // The shared lease covers binding, staging and registry commit.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaInstallError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaInstallError::Registry)?;
    }
    install_owned_media_batch_unlocked(store, registry, assets, kind, owner_id)
}

/// Caller must hold a SHARED media lease and validate registry-root binding
/// before calling. The manifest installer uses this to keep its single lease
/// alive through post-commit verification without recursively locking SQLite.
pub(crate) fn install_owned_media_batch_unlocked(
    store: &MediaObjectStore,
    registry: &mut MediaRegistry,
    assets: &[(&[u8], &MediaNotice)],
    kind: OwnerKind,
    owner_id: &str,
) -> Result<Vec<String>, MediaInstallError> {
    let mut staged = Vec::with_capacity(assets.len());
    for &(content, _) in assets {
        staged.push(
            store
                .store_bytes_unlocked(content)
                .map_err(MediaInstallError::Store)?,
        );
    }
    let registered = registry
        .register_verified_owned_batch(assets, kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    if registered != staged {
        return Err(MediaInstallError::IdentityMismatch);
    }
    Ok(staged)
}

/// Rehash every registered object for one owner. An authenticated pack
/// manifest must separately prove that this owner has *all* required media.
pub fn verify_registered_owner_bytes(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
) -> Result<u64, MediaInstallError> {
    verify_registered_owner_bytes_with_binding(store, registry, kind, owner_id, false)
}

/// Rehash a complete owner inventory only after verifying its registry
/// belongs to the media root. This is an integrity check, not proof that an
/// authenticated pack manifest declared every required visual.
pub fn verify_bound_registered_owner_bytes(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
) -> Result<u64, MediaInstallError> {
    verify_registered_owner_bytes_with_binding(store, registry, kind, owner_id, true)
}

fn verify_registered_owner_bytes_with_binding(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    require_binding: bool,
) -> Result<u64, MediaInstallError> {
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaInstallError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaInstallError::Registry)?;
    }
    let claims = registry
        .owned_media(kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    let entries: Vec<RequiredMedia> = claims
        .into_iter()
        .map(|(digest, bytes)| RequiredMedia { digest, bytes })
        .collect();
    verify_declared_media_unlocked(store, &entries).map_err(MediaInstallError::Inventory)
}

/// Compare an authenticated manifest's required-media inventory with exactly
/// the owner's registered objects, then rehash every on-disk object. Neither
/// this method nor registry membership authenticates the manifest itself or
/// proves the caller discovered every visual used by the article renderer.
pub fn verify_required_owner_media(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    required: &[RequiredMedia],
) -> Result<u64, MediaInstallError> {
    verify_required_owner_media_with_binding(store, registry, kind, owner_id, required, false)
}

/// Production manifest-to-owner verification: validate the registry's media
/// root under the same SHARED lease as the exact inventory comparison and
/// streamed SHA-256 reads. This does not authenticate the supplied manifest.
pub fn verify_bound_required_owner_media(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    required: &[RequiredMedia],
) -> Result<u64, MediaInstallError> {
    verify_required_owner_media_with_binding(store, registry, kind, owner_id, required, true)
}

fn verify_required_owner_media_with_binding(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    required: &[RequiredMedia],
    require_binding: bool,
) -> Result<u64, MediaInstallError> {
    // Hold the lease across binding, owner lookup, inventory comparison and
    // every digest read. The unlocked verifier avoids nested SQLite locks.
    let _lease = MediaLeaseCoordinator::new(store.root())
        .acquire_shared()
        .map_err(MediaInstallError::Lease)?;
    if require_binding {
        registry
            .require_media_root_binding(store.root())
            .map_err(MediaInstallError::Registry)?;
    }
    verify_required_owner_media_unlocked(store, registry, kind, owner_id, required)
}

/// Caller must already hold a SHARED media lease. The manifest installer
/// verifies the committed owner inventory without releasing its install lease.
pub(crate) fn verify_required_owner_media_unlocked(
    store: &MediaObjectStore,
    registry: &MediaRegistry,
    kind: OwnerKind,
    owner_id: &str,
    required: &[RequiredMedia],
) -> Result<u64, MediaInstallError> {
    let registered = registry
        .owned_media(kind, owner_id)
        .map_err(MediaInstallError::Registry)?;
    if registered.len() != required.len() {
        return Err(MediaInstallError::IdentityMismatch);
    }
    let mut expected: Vec<_> = required
        .iter()
        .map(|entry| (entry.digest.as_str(), entry.bytes))
        .collect();
    expected.sort_unstable();
    if !registered
        .iter()
        .map(|(digest, bytes)| (digest.as_str(), *bytes))
        .eq(expected.into_iter())
    {
        return Err(MediaInstallError::IdentityMismatch);
    }
    verify_declared_media_unlocked(store, required).map_err(MediaInstallError::Inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_inventory::{verify_declared_media, MediaInventoryError, RequiredMedia};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn notice(creator: &str) -> MediaNotice {
        MediaNotice {
            mime: "image/svg+xml".into(),
            source_url: "https://commons.wikimedia.org/wiki/File:Diagram.svg".into(),
            creator: creator.into(),
            license: "CC BY-SA 4.0".into(),
        }
    }

    #[test]
    fn bound_single_install_rolls_back_metadata_when_owner_insert_fails() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-atomic-single-media-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let database = root.with_extension("registry.sqlite");
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::open(&database).unwrap();
        registry.bind_media_root(&root).unwrap();
        let attribution = notice("Creator A");

        // Incremental installs must still allow several objects for one owner.
        for bytes in [b"first diagram".as_slice(), b"second diagram".as_slice()] {
            install_bound_owned_media(
                &store,
                &mut registry,
                bytes,
                &attribution,
                OwnerKind::Pack,
                "incremental-pack",
            )
            .unwrap();
        }
        assert_eq!(
            registry
                .owned_media(OwnerKind::Pack, "incremental-pack")
                .unwrap()
                .len(),
            2
        );
        let raw = rusqlite::Connection::open(&database).unwrap();
        let count = |table: &str| -> i64 {
            raw.query_row(
                &format!("SELECT COUNT(*) FROM {table}"),
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        let objects_before = count("media_objects");
        let notices_before = count("media_notices");
        let owners_before = count("media_owners");

        // Force SQLite to fail after object/notice insertion, before commit.
        raw.execute_batch(
            "CREATE TRIGGER deny_single_owner BEFORE INSERT ON media_owners
             WHEN NEW.owner_id='denied-pack'
             BEGIN SELECT RAISE(ABORT, 'simulated owner insert failure'); END;",
        )
        .unwrap();
        assert!(matches!(
            install_bound_owned_media(
                &store,
                &mut registry,
                b"third diagram",
                &notice("Creator B"),
                OwnerKind::Pack,
                "denied-pack",
            ),
            Err(MediaInstallError::Registry(MediaRegistryError::Sql(_)))
        ));
        assert_eq!(count("media_objects"), objects_before);
        assert_eq!(count("media_notices"), notices_before);
        assert_eq!(count("media_owners"), owners_before);
        assert!(registry
            .owned_media(OwnerKind::Pack, "denied-pack")
            .unwrap()
            .is_empty());
        // File publication precedes SQLite; the orphan remains recoverable.
        assert_eq!(store.published_object_digests(10).unwrap().len(), 3);

        raw.execute_batch("DROP TRIGGER deny_single_owner;").unwrap();
        install_bound_owned_media(
            &store,
            &mut registry,
            b"third diagram",
            &notice("Creator B"),
            OwnerKind::Pack,
            "denied-pack",
        )
        .unwrap();
        assert_eq!(count("media_objects"), objects_before + 1);
        assert_eq!(count("media_notices"), notices_before + 1);
        assert_eq!(count("media_owners"), owners_before + 1);
        drop(raw);
        drop(registry);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(database).unwrap();
    }

    #[test]
    fn bound_installers_reject_unbound_and_wrong_root_before_any_publication() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-bound-install-{}-{stamp}", std::process::id()));
        let other = std::env::temp_dir().join(format!(
            "wiki-bound-install-other-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(&other).unwrap();
        let database = root.with_extension("registry.sqlite");
        let store = MediaObjectStore::new(&root);
        let wrong_store = MediaObjectStore::new(&other);
        let mut registry = MediaRegistry::open(&database).unwrap();
        let author = notice("Creator A");
        let assets: &[(&[u8], &MediaNotice)] = &[
            (b"first bound diagram", &author),
            (b"second bound diagram", &author),
        ];

        assert!(matches!(
            install_bound_owned_media(
                &store,
                &mut registry,
                b"first bound diagram",
                &author,
                OwnerKind::Pack,
                "math",
            ),
            Err(MediaInstallError::Registry(MediaRegistryError::UnboundRoot))
        ));
        assert!(matches!(
            install_bound_owned_media_batch(&store, &mut registry, assets, OwnerKind::Pack, "math",),
            Err(MediaInstallError::Registry(MediaRegistryError::UnboundRoot))
        ));
        assert!(store.published_object_digests(10).unwrap().is_empty());
        assert!(registry
            .owned_media(OwnerKind::Pack, "math")
            .unwrap()
            .is_empty());

        // Bind the empty store explicitly, then reject a different valid
        // media root with the same database before staging any bytes.
        registry.bind_media_root(&root).unwrap();
        assert!(matches!(
            install_bound_owned_media(
                &wrong_store,
                &mut registry,
                b"first bound diagram",
                &author,
                OwnerKind::Pack,
                "math",
            ),
            Err(MediaInstallError::Registry(
                MediaRegistryError::RootMismatch
            ))
        ));
        assert!(matches!(
            install_bound_owned_media_batch(
                &wrong_store,
                &mut registry,
                assets,
                OwnerKind::Pack,
                "math",
            ),
            Err(MediaInstallError::Registry(
                MediaRegistryError::RootMismatch
            ))
        ));
        assert!(wrong_store.published_object_digests(10).unwrap().is_empty());

        let digests =
            install_bound_owned_media_batch(&store, &mut registry, assets, OwnerKind::Pack, "math")
                .unwrap();
        assert_eq!(digests.len(), 2);
        let duplicate = install_bound_owned_media(
            &store,
            &mut registry,
            b"first bound diagram",
            &author,
            OwnerKind::UserPin,
            "saved",
        )
        .unwrap();
        assert_eq!(duplicate, digests[0]);
        assert_eq!(store.published_object_digests(10).unwrap().len(), 2);
        drop(registry);

        // Binding and ownership survive a separate SQLite connection.
        let registry = MediaRegistry::open_existing(&database).unwrap();
        registry.require_media_root_binding(&root).unwrap();
        assert_eq!(
            registry.owned_media(OwnerKind::Pack, "math").unwrap().len(),
            2
        );
        assert_eq!(
            registry
                .owned_media(OwnerKind::UserPin, "saved")
                .unwrap()
                .len(),
            1
        );
        let owned = read_verified_bound_owned_media(
            &store,
            &registry,
            OwnerKind::Pack,
            "math",
            &digests[0],
        )
        .unwrap()
        .unwrap();
        assert_eq!(owned.bytes, b"first bound diagram");
        assert_eq!(owned.notices, vec![author.clone()]);
        assert!(matches!(
            read_verified_bound_owned_media(
                &wrong_store,
                &registry,
                OwnerKind::Pack,
                "math",
                &digests[0],
            ),
            Err(MediaInstallError::Registry(
                MediaRegistryError::RootMismatch
            ))
        ));
        let required = vec![
            RequiredMedia {
                digest: digests[0].clone(),
                bytes: b"first bound diagram".len() as u64,
            },
            RequiredMedia {
                digest: digests[1].clone(),
                bytes: b"second bound diagram".len() as u64,
            },
        ];
        let total = required.iter().map(|entry| entry.bytes).sum::<u64>();
        assert_eq!(
            verify_bound_registered_owner_bytes(&store, &registry, OwnerKind::Pack, "math")
                .unwrap(),
            total
        );
        assert_eq!(
            verify_bound_required_owner_media(
                &store,
                &registry,
                OwnerKind::Pack,
                "math",
                &required,
            )
            .unwrap(),
            total
        );
        assert!(matches!(
            verify_bound_required_owner_media(
                &wrong_store,
                &registry,
                OwnerKind::Pack,
                "math",
                &required,
            ),
            Err(MediaInstallError::Registry(
                MediaRegistryError::RootMismatch
            ))
        ));
        drop(registry);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(other).unwrap();
        fs::remove_file(database).unwrap();
    }

    #[test]
    fn verified_owned_reader_enforces_owner_attribution_and_byte_integrity() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-owned-read-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let db = root.join("owners.sqlite");
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::open(&db).unwrap();
        let attribution = notice("Creator A");
        let digest = install_owned_media(
            &store,
            &mut registry,
            b"verified visual",
            &attribution,
            OwnerKind::Pack,
            "science",
        )
        .unwrap();

        let owned =
            read_verified_owned_media(&store, &registry, OwnerKind::Pack, "science", &digest)
                .unwrap()
                .unwrap();
        assert_eq!(owned.bytes, b"verified visual");
        assert_eq!(owned.notices, vec![attribution]);
        assert!(read_verified_owned_media(
            &store,
            &registry,
            OwnerKind::Pack,
            "another-pack",
            &digest,
        )
        .unwrap()
        .is_none());
        assert!(read_verified_owned_media(
            &store,
            &registry,
            OwnerKind::UserPin,
            "science",
            &digest,
        )
        .unwrap()
        .is_none());

        // A corrupt attribution row must never reach a reader together with
        // otherwise correctly hashed media bytes.
        let raw = rusqlite::Connection::open(&db).unwrap();
        raw.execute(
            "UPDATE media_notices SET creator='bad' || char(10) WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        assert!(matches!(
            read_verified_owned_media(&store, &registry, OwnerKind::Pack, "science", &digest),
            Err(MediaInstallError::Registry(
                MediaRegistryError::InvalidInput
            ))
        ));
        raw.execute(
            "UPDATE media_notices SET creator='Creator A' WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();

        // Registry size corruption is rejected after the actual file read.
        raw.execute(
            "UPDATE media_objects SET bytes=bytes+1 WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        assert!(matches!(
            read_verified_owned_media(&store, &registry, OwnerKind::Pack, "science", &digest),
            Err(MediaInstallError::IdentityMismatch)
        ));
        raw.execute(
            "UPDATE media_objects SET bytes=bytes-1 WHERE digest=?1",
            rusqlite::params![digest],
        )
        .unwrap();
        fs::write(root.join(&digest[..2]).join(&digest), b"tampered visual").unwrap();
        assert!(matches!(
            read_verified_owned_media(&store, &registry, OwnerKind::Pack, "science", &digest),
            Err(MediaInstallError::Store(MediaObjectError::DigestMismatch))
        ));
        fs::write(root.join(&digest[..2]).join(&digest), b"verified visual").unwrap();

        registry
            .remove_owner(&digest, OwnerKind::Pack, "science")
            .unwrap();
        assert!(
            read_verified_owned_media(&store, &registry, OwnerKind::Pack, "science", &digest,)
                .unwrap()
                .is_none()
        );
        drop(raw);
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_owned_reader_waits_for_exclusive_gc_lease() {
        use std::sync::mpsc;
        use std::time::Duration;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-owned-read-lease-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let db = root.join("owners.sqlite");
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::open(&db).unwrap();
        let digest = install_owned_media(
            &store,
            &mut registry,
            b"lease protected media",
            &notice("Author"),
            OwnerKind::Pack,
            "science",
        )
        .unwrap();
        let exclusive = MediaLeaseCoordinator::new(&root)
            .acquire_exclusive()
            .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker_root = root.clone();
        let worker_db = db.clone();
        let worker_digest = digest.clone();
        let worker = std::thread::spawn(move || {
            let store = MediaObjectStore::new(worker_root);
            let registry = MediaRegistry::open(&worker_db).unwrap();
            started_tx.send(()).unwrap();
            finished_tx
                .send(read_verified_owned_media(
                    &store,
                    &registry,
                    OwnerKind::Pack,
                    "science",
                    &worker_digest,
                ))
                .unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(finished_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        drop(exclusive);
        let media = finished_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(media.bytes, b"lease protected media");
        worker.join().unwrap();
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusive_gc_lease_blocks_installer_until_released() {
        use std::sync::mpsc;
        use std::time::Duration;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-install-lease-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let database = root.join("owners.sqlite");
        let registry = MediaRegistry::open(&database).unwrap();
        let exclusive = MediaLeaseCoordinator::new(&root)
            .acquire_exclusive()
            .unwrap();

        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker_root = root.clone();
        let worker_db = database.clone();
        let worker = std::thread::spawn(move || {
            let store = MediaObjectStore::new(&worker_root);
            let mut registry = MediaRegistry::open(&worker_db).unwrap();
            started_tx.send(()).unwrap();
            finished_tx
                .send(install_owned_media(
                    &store,
                    &mut registry,
                    b"protected bytes",
                    &notice("Author"),
                    OwnerKind::Pack,
                    "science",
                ))
                .unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(finished_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err());
        assert!(registry
            .owned_media(OwnerKind::Pack, "science")
            .unwrap()
            .is_empty());

        drop(exclusive);
        let digest = finished_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(
            registry
                .owned_digest_size(OwnerKind::Pack, "science", &digest)
                .unwrap(),
            Some(b"protected bytes".len() as u64)
        );
        assert_eq!(
            MediaObjectStore::new(&root).read_verified(&digest).unwrap(),
            b"protected bytes"
        );
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_ownership_is_atomic_and_persists_across_reopen() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("wiki-batch-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let db = root.join("owners.sqlite");
        let a = notice("Creator A");
        let b = notice("Creator B");
        let bad = MediaNotice {
            license: " ".into(),
            ..notice("Bad")
        };
        let mut registry = MediaRegistry::open(&db).unwrap();
        let first: &[(&[u8], &MediaNotice)] = &[(b"diagram", &a), (b"figure", &bad)];
        assert!(matches!(
            install_owned_media_batch(&store, &mut registry, first, OwnerKind::Pack, "physics"),
            Err(MediaInstallError::Registry(
                MediaRegistryError::InvalidInput
            ))
        ));
        assert!(registry
            .owned_media(OwnerKind::Pack, "physics")
            .unwrap()
            .is_empty());
        assert!(registry.unowned_digests().unwrap().is_empty());

        let assets: &[(&[u8], &MediaNotice)] =
            &[(b"diagram", &a), (b"figure", &b), (b"diagram", &b)];
        let digests =
            install_owned_media_batch(&store, &mut registry, assets, OwnerKind::Pack, "physics")
                .unwrap();
        assert_eq!(digests[0], digests[2]);
        assert_eq!(registry.notice_count(&digests[0]).unwrap(), 2);
        assert_eq!(
            registry
                .owned_media(OwnerKind::Pack, "physics")
                .unwrap()
                .len(),
            2
        );
        registry
            .add_owner(&digests[0], OwnerKind::UserPin, "reader")
            .unwrap();
        drop(registry);

        let registry = MediaRegistry::open(&db).unwrap();
        let required = vec![
            RequiredMedia {
                digest: digests[0].clone(),
                bytes: 7,
            },
            RequiredMedia {
                digest: digests[1].clone(),
                bytes: 6,
            },
        ];
        assert_eq!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required)
                .unwrap(),
            13
        );
        registry
            .remove_owner(&digests[0], OwnerKind::Pack, "physics")
            .unwrap();
        registry
            .remove_owner(&digests[1], OwnerKind::Pack, "physics")
            .unwrap();
        assert_eq!(
            registry.unowned_digests().unwrap(),
            vec![digests[1].clone()]
        );
        assert_eq!(store.read_verified(&digests[0]).unwrap(), b"diagram");
        drop(registry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_media_is_shared_and_ownership_survives_pack_removal() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-install-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        let digest = install_owned_media(
            &store,
            &mut registry,
            b"diagram",
            &notice("Creator A"),
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        assert_eq!(
            install_owned_media(
                &store,
                &mut registry,
                b"diagram",
                &notice("Creator B"),
                OwnerKind::Pack,
                "math",
            )
            .unwrap(),
            digest
        );
        assert_eq!(registry.notice_count(&digest).unwrap(), 2);
        registry
            .remove_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        assert!(registry.unowned_digests().unwrap().is_empty());
        assert_eq!(
            verify_registered_owner_bytes(&store, &registry, OwnerKind::Pack, "math").unwrap(),
            7
        );
        assert_eq!(
            verify_declared_media(
                &store,
                &[RequiredMedia {
                    digest: digest.clone(),
                    bytes: 7,
                }]
            )
            .unwrap(),
            7
        );
        registry
            .remove_owner(&digest, OwnerKind::Pack, "math")
            .unwrap();
        assert_eq!(registry.unowned_digests().unwrap(), vec![digest.clone()]);
        fs::write(root.join(&digest[..2]).join(&digest), b"changed").unwrap();
        assert!(matches!(
            verify_declared_media(&store, &[RequiredMedia { digest, bytes: 7 }]),
            Err(MediaInventoryError::Unreadable(_))
        ));
        assert!(matches!(
            verify_registered_owner_bytes(&store, &registry, OwnerKind::Pack, "physics"),
            Ok(0)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn required_media_must_match_owner_claims_before_acceptance() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-required-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        let shared = install_owned_media(
            &store,
            &mut registry,
            b"diagram",
            &notice("Creator"),
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        let additional = install_owned_media(
            &store,
            &mut registry,
            b"figure",
            &notice("Creator"),
            OwnerKind::Pack,
            "physics",
        )
        .unwrap();
        let required = vec![
            RequiredMedia {
                digest: additional.clone(),
                bytes: 6,
            },
            RequiredMedia {
                digest: shared.clone(),
                bytes: 7,
            },
        ];
        assert_eq!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required)
                .unwrap(),
            13
        );
        assert!(matches!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "missing", &required),
            Err(MediaInstallError::IdentityMismatch)
        ));
        assert!(matches!(
            verify_required_owner_media(
                &store,
                &registry,
                OwnerKind::Pack,
                "physics",
                &required[..1]
            ),
            Err(MediaInstallError::IdentityMismatch)
        ));
        let mut wrong_size = required.clone();
        wrong_size[0].bytes = 9;
        assert!(matches!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &wrong_size),
            Err(MediaInstallError::IdentityMismatch)
        ));
        fs::write(root.join(&shared[..2]).join(&shared), b"tampered").unwrap();
        assert!(matches!(
            verify_required_owner_media(&store, &registry, OwnerKind::Pack, "physics", &required),
            Err(MediaInstallError::Inventory(
                MediaInventoryError::Unreadable(_)
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_owner_never_becomes_an_installed_owner() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-install-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let mut registry = MediaRegistry::in_memory().unwrap();
        assert!(matches!(
            install_owned_media(
                &store,
                &mut registry,
                b"diagram",
                &notice("Creator"),
                OwnerKind::Pack,
                "../invalid",
            ),
            Err(MediaInstallError::Registry(
                MediaRegistryError::InvalidInput
            ))
        ));
        assert!(registry.unowned_digests().unwrap().is_empty());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
