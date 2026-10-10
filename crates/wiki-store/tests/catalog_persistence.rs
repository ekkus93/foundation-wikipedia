use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_model::{ArticleKey, PageRecord, Redirect};
use wiki_store::catalog::{CatalogError, SnapshotCatalog};
use wiki_store::record_codec::append_record_frame;

#[test]
fn sqlite_catalog_survives_reopen_and_detects_corrupt_shard() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-persist-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let from = ArticleKey { project: "enwiki".into(), page_id: 12 };
    let record = PageRecord::Redirect(Redirect {
        from: from.clone(),
        title: "Earth alias".into(),
        to: ArticleKey { project: "enwiki".into(), page_id: 42 },
    });
    let mut frame = Vec::new();
    append_record_frame(&mut frame, &record).unwrap();
    let shard = root.join("data.shard");
    fs::write(&shard, &frame).unwrap();
    let database = root.join("catalog.sqlite");
    {
        let mut catalog = SnapshotCatalog::open(&database).unwrap();
        catalog.insert_verified(&root, "data.shard", 0, frame.len() as u64).unwrap();
    }
    let catalog = SnapshotCatalog::open(&database).unwrap();
    assert_eq!(catalog.lookup_key("enwiki", "Earth_alias").unwrap(), Some(from.clone()));
    assert_eq!(catalog.read_record(&root, &from).unwrap(), Some(record));
    let mut damaged = frame.clone();
    damaged[12] ^= 1;
    fs::write(&shard, &damaged).unwrap();
    assert!(matches!(catalog.read_record(&root, &from), Err(CatalogError::DigestMismatch)));
    fs::remove_file(&shard).unwrap();
    assert!(matches!(catalog.read_record(&root, &from), Err(CatalogError::Io(_))));
    drop(catalog);
    fs::remove_dir_all(root).unwrap();
}
