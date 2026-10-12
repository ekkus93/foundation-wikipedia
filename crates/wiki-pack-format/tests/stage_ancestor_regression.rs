//! Regression: an unsigned pack must not be staged through a symlinked
//! ancestor even if the final destination directory is itself real.
#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::symlink;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_pack_format::container::{write_container, ContainerError};
use wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};
use wiki_pack_format::stage::stage_container;

#[test]
fn staged_pack_rejects_symlinked_parent_ancestor_without_publishing() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("wpack-stage-ancestor-{}-{nonce}", std::process::id()));
    let actual = root.join("actual");
    let nested = actual.join("private");
    fs::create_dir_all(&nested).unwrap();
    let alias = root.join("alias");
    symlink(&actual, &alias).unwrap();

    let payload = b"article";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "test".into(),
        project: "enwiki".into(),
        snapshot: "20261011".into(),
        origin: Origin::Custom { definition_id: "test".into() },
        objects: vec![Object {
            path: "articles/1.pack".into(),
            sha256: format!("{:x}", Sha256::digest(payload)),
            bytes: payload.len() as u64,
        }],
    };
    let mut envelope = Vec::new();
    write_container(&mut envelope, &manifest, |_, output| output.write_all(payload)).unwrap();
    let rejected = stage_container(&mut std::io::Cursor::new(envelope), &alias.join("private"), "staged");
    assert!(matches!(rejected, Err(ContainerError::UnsafeStagingPath)));
    assert!(!nested.join("staged").exists());
    fs::remove_dir_all(root).unwrap();
}
