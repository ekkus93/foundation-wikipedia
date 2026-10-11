#[cfg(unix)]
#[test]
fn new_offline_index_is_owner_only() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiki_store::search_index::OfflineSearchIndex;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wiki-private-index-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let path = root.join("index.sqlite");
    let index = OfflineSearchIndex::open(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(index);
    fs::remove_dir_all(root).unwrap();
}
