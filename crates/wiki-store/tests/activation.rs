use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_store::activation::{ActivationError, ActiveSnapshotStore};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "wiki-snapshot-test-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("snapshots")).unwrap();
        Self(dir)
    }

    fn stage(&self, id: &str) {
        let folder = self.0.join("snapshots").join(id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("manifest.json"), b"{\"fixture\":true}").unwrap();
    }

    fn store(&self) -> ActiveSnapshotStore {
        ActiveSnapshotStore::new(&self.0)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn verified_candidates_switch_atomically_and_preserve_old_snapshot() {
    let root = TestRoot::new();
    root.stage("old-1");
    root.stage("new-2");
    let store = root.store();
    assert_eq!(store.current().unwrap(), None);
    store.switch_to_prevalidated("old-1").unwrap();
    assert_eq!(store.current().unwrap(), Some("old-1".into()));
    store.switch_to_prevalidated("new-2").unwrap();
    assert_eq!(store.current().unwrap(), Some("new-2".into()));
    assert!(root.0.join("snapshots/old-1/manifest.json").exists());
    assert_eq!(fs::read_to_string(root.0.join("current")).unwrap(), "new-2\n");
}

#[test]
fn rejected_candidate_keeps_previous_snapshot_and_pointer() {
    let root = TestRoot::new();
    root.stage("working");
    let store = root.store();
    store.switch_to_prevalidated("working").unwrap();
    assert!(matches!(
        store.switch_to_prevalidated("../escape"),
        Err(ActivationError::InvalidSnapshotId)
    ));
    assert!(matches!(
        store.switch_to_prevalidated("not-present"),
        Err(ActivationError::CandidateNotStaged)
    ));
    let empty = root.0.join("snapshots/unverified");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        store.switch_to_prevalidated("unverified"),
        Err(ActivationError::CandidateNotStaged)
    ));
    assert_eq!(store.current().unwrap(), Some("working".into()));
}

#[test]
fn corrupted_active_pointer_is_never_silently_accepted() {
    let root = TestRoot::new();
    root.stage("good");
    let store = root.store();
    store.switch_to_prevalidated("good").unwrap();
    fs::write(root.0.join("current"), "../escape\n").unwrap();
    assert!(matches!(
        store.current(),
        Err(ActivationError::CorruptCurrentPointer)
    ));
}

#[cfg(unix)]
#[test]
fn symlinked_candidate_is_rejected() {
    use std::os::unix::fs::symlink;
    let root = TestRoot::new();
    root.stage("safe");
    let store = root.store();
    store.switch_to_prevalidated("safe").unwrap();
    symlink(root.0.join("snapshots/safe"), root.0.join("snapshots/alias")).unwrap();
    assert!(matches!(
        store.switch_to_prevalidated("alias"),
        Err(ActivationError::CandidateNotStaged)
    ));
    assert_eq!(store.current().unwrap(), Some("safe".into()));
}
