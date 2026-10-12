//! Regression: staging rejects a symlinked parent ancestor.
#![cfg(unix)]
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::symlink;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_pack_format::container::{write_container, ContainerError};
use wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};
use wiki_pack_format::stage::stage_container;

#[test]
fn rejects_symlinked_staging_parent_without_publishing() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "wpack-stage-parent-{}-{stamp}",
        std::process::id()
    ));
    let real = root.join("real");
    let parent = real.join("private");
    fs::create_dir_all(&parent).unwrap();
    symlink(&real, root.join("alias")).unwrap();
    let payload = b"article";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "test".into(),
        project: "enwiki".into(),
        snapshot: "20261011".into(),
        origin: Origin::Custom {
            definition_id: "test".into(),
        },
        objects: vec![Object {
            path: "articles/1.pack".into(),
            sha256: format!("{:x}", Sha256::digest(payload)),
            bytes: payload.len() as u64,
        }],
    };
    let mut envelope = Vec::new();
    write_container(&mut envelope, &manifest, |_, out| {
        out.write_all(payload)
    })
    .unwrap();
    let result = stage_container(
        &mut std::io::Cursor::new(envelope),
        &root.join("alias/private"),
        "staged",
    );
    assert!(matches!(result, Err(ContainerError::UnsafeStagingPath)));
    assert!(!parent.join("staged").exists());
    fs::remove_dir_all(root).unwrap();
}
