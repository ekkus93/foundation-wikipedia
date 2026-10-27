//! Advisory cross-process media-operation leases backed by SQLite rollback
//! journal locks. This is NOT filesystem deletion authorization: every reader
//! and publisher must participate before production GC may unlink bytes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use crate::media_ownership::{prepare_registry_path, MediaRegistryError};

#[derive(Debug)]
pub enum MediaLeaseError {
    RegistryPath(MediaRegistryError),
    Sql(rusqlite::Error),
    UnsafeJournalMode,
}

pub struct MediaLeaseCoordinator {
    path: PathBuf,
}

/// Dropping this connection releases its OS-backed lock, including on process
/// termination. Never use PID sentinel files for this protocol.
pub struct MediaLease {
    _connection: Connection,
}

impl MediaLeaseCoordinator {
    pub fn new(root: &Path) -> Self {
        Self {
            path: root.join(".media-lease.sqlite"),
        }
    }

    pub fn acquire_shared(&self) -> Result<MediaLease, MediaLeaseError> {
        self.acquire(false)
    }

    pub fn acquire_exclusive(&self) -> Result<MediaLease, MediaLeaseError> {
        self.acquire(true)
    }

    fn acquire(&self, exclusive: bool) -> Result<MediaLease, MediaLeaseError> {
        // Multiple first-time readers may race to create the same private
        // lock database. The registry path preflight deliberately rejects
        // create_new(AlreadyExists); retry only after a fresh full path audit.
        // Unsafe symlinks, hardlinks and traversal remain rejected.
        let mut ready = false;
        for _ in 0..3 {
            match prepare_registry_path(&self.path) {
                Ok(()) => {
                    ready = true;
                    break;
                }
                Err(MediaRegistryError::UnsafePath) => continue,
                Err(error) => return Err(MediaLeaseError::RegistryPath(error)),
            }
        }
        if !ready {
            return Err(MediaLeaseError::RegistryPath(MediaRegistryError::UnsafePath));
        }
        let conn = Connection::open(&self.path).map_err(MediaLeaseError::Sql)?;
        conn.busy_timeout(Duration::from_secs(10))
            .map_err(MediaLeaseError::Sql)?;
        // WAL readers do not block writers and cannot enforce this protocol.
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(MediaLeaseError::Sql)?;
        if mode != "delete" {
            return Err(MediaLeaseError::UnsafeJournalMode);
        }
        if exclusive {
            conn.execute_batch("BEGIN EXCLUSIVE;")
                .map_err(MediaLeaseError::Sql)?;
        } else {
            // BEGIN DEFERRED alone does not take a lock; the SELECT forces a
            // SHARED lock that remains held until this connection is closed.
            conn.execute_batch("BEGIN DEFERRED; SELECT COUNT(*) FROM sqlite_master;")
                .map_err(MediaLeaseError::Sql)?;
        }
        Ok(MediaLease { _connection: conn })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-media-lease-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn shared_leases_exclude_exclusive_and_release_after_drop() {
        let root = root();
        let coordinator = MediaLeaseCoordinator::new(&root);
        let first = coordinator.acquire_shared().unwrap();
        let second = coordinator.acquire_shared().unwrap();
        let contender = Connection::open(&coordinator.path).unwrap();
        contender.busy_timeout(Duration::from_millis(0)).unwrap();
        assert!(contender.execute_batch("BEGIN EXCLUSIVE;").is_err());
        drop(first);
        assert!(contender.execute_batch("BEGIN EXCLUSIVE;").is_err());
        drop(second);
        contender
            .execute_batch("BEGIN EXCLUSIVE; ROLLBACK;")
            .unwrap();
        let exclusive = coordinator.acquire_exclusive().unwrap();
        assert!(contender
            .execute_batch("BEGIN DEFERRED; SELECT COUNT(*) FROM sqlite_master;")
            .is_err());
        contender.execute_batch("ROLLBACK;").unwrap();
        drop(exclusive);
        contender
            .execute_batch("BEGIN DEFERRED; SELECT COUNT(*) FROM sqlite_master; ROLLBACK;")
            .unwrap();
        drop(contender);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_first_open_revalidates_private_lock_file() {
        use std::sync::{Arc, Barrier};
        let root = root();
        let barrier = Arc::new(Barrier::new(8));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let path = root.clone();
            let ready = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                ready.wait();
                let coordinator = MediaLeaseCoordinator::new(&path);
                let _lease = coordinator.acquire_shared().unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lock_file_is_private_and_media_inventory_skips_it() {
        use crate::media_objects::MediaObjectStore;
        let root = root();
        let coordinator = MediaLeaseCoordinator::new(&root);
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"real media object").unwrap();
        let lease = coordinator.acquire_shared().unwrap();
        assert_eq!(store.published_object_digests(2).unwrap(), vec![digest]);
        drop(lease);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&coordinator.path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn hardlink_alias_is_rejected_before_lock_acquisition() {
        let root = root();
        let coordinator = MediaLeaseCoordinator::new(&root);
        drop(coordinator.acquire_shared().unwrap());
        let alias = root.join("other.sqlite");
        fs::hard_link(&coordinator.path, &alias).unwrap();
        assert!(matches!(
            coordinator.acquire_shared(),
            Err(MediaLeaseError::RegistryPath(
                MediaRegistryError::UnsafePath
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
