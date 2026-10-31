//! Black-box staged-object validation: local integrity is not publisher trust.
use sha2::{Digest, Sha256};
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin};

fn fixture() -> Vec<u8> {
    let manifest = Manifest {
        version: 1,
        pack_id: "science".into(),
        project: "enwiki".into(),
        snapshot: "20261030".into(),
        origin: Origin::Official {
            publisher: "unverified-publisher".into(),
        },
        objects: vec![Object {
            path: "articles/1.cbor".into(),
            sha256: format!("{:x}", Sha256::digest(b"article")),
            bytes: 7,
        }],
    };
    let mut bytes = Vec::new();
    write_container(&mut bytes, &manifest, |_, output| {
        output.write_all(b"article")
    })
    .unwrap();
    bytes
}

fn parent() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("fwpack-tree-check-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    root
}

#[test]
fn untrusted_tree_verification_rejects_tampering_and_extras_without_mutating_tree() {
    let root = parent();
    let archive = root.with_extension("wpack");
    let bytes = fixture();
    fs::write(&archive, &bytes).unwrap();
    let staged = root.join("candidate");
    let binary = env!("CARGO_BIN_EXE_wiki-pack");
    let stage = Command::new(binary)
        .arg("stage-unsigned")
        .arg("--json")
        .arg(&archive)
        .arg(&root)
        .arg("candidate")
        .output()
        .unwrap();
    assert!(stage.status.success(), "{stage:?}");
    let receipt: serde_json::Value = serde_json::from_slice(&stage.stdout).unwrap();

    let verify = || {
        Command::new(binary)
            .arg("verify-staged")
            .arg("--json")
            .arg(&archive)
            .arg(&staged)
            .output()
            .unwrap()
    };
    let valid = verify();
    assert!(valid.status.success(), "{valid:?}");
    let json: serde_json::Value = serde_json::from_slice(&valid.stdout).unwrap();
    assert_eq!(json["status"], "untrusted_staged_tree_verification");
    assert_eq!(json["manifest_sha256"], receipt["manifest_sha256"]);
    assert_eq!(json["staging_path"], receipt["staging_path"]);
    assert_eq!(json["measured_payload_bytes"], 7);
    assert_eq!(json["staged_tree_verified"], true);
    assert_eq!(json["integrity_verified"], true);
    assert_eq!(json["publisher_signature_verified"], false);
    assert_eq!(json["offline_media_complete"], false);
    assert_eq!(json["installed"], false);
    assert_eq!(json["claimed_origin"], "official");
    let human = Command::new(binary)
        .arg("verify-staged")
        .arg(&archive)
        .arg(&staged)
        .output()
        .unwrap();
    assert!(human.status.success(), "{human:?}");
    assert!(String::from_utf8_lossy(&human.stdout).contains("NOT INSTALLED"));

    let limited = Command::new(binary)
        .arg("verify-staged")
        .arg("--json")
        .arg(&archive)
        .arg(&staged)
        .arg((bytes.len() - 1).to_string())
        .output()
        .unwrap();
    assert_eq!(limited.status.code(), Some(1));
    assert!(limited.stdout.is_empty());

    let object = staged.join("articles/1.cbor");
    fs::write(&object, b"ARTICLE").unwrap();
    let corrupt = verify();
    assert_eq!(corrupt.status.code(), Some(1), "{corrupt:?}");
    assert!(corrupt.stdout.is_empty());
    fs::write(&object, b"article").unwrap();
    fs::write(staged.join("unlisted.txt"), b"unexpected").unwrap();
    let extra = verify();
    assert_eq!(extra.status.code(), Some(1), "{extra:?}");
    assert!(extra.stdout.is_empty());
    fs::remove_file(staged.join("unlisted.txt")).unwrap();
    assert!(verify().status.success());
    assert_eq!(fs::read(&object).unwrap(), b"article");
    for args in [
        vec!["verify-staged", "--json"],
        vec!["verify-staged", "--json", "x", "y", "invalid-budget"],
        vec!["verify-staged", "--json", "x", "y", "1", "excess"],
    ] {
        let bad = Command::new(binary).args(args).output().unwrap();
        assert_eq!(bad.status.code(), Some(2), "{bad:?}");
        assert!(bad.stdout.is_empty());
    }
    fs::remove_file(archive).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_tree_is_never_accepted_as_verified_staging() {
    use std::os::unix::fs::symlink;
    let root = parent();
    let archive = root.with_extension("wpack");
    fs::write(&archive, fixture()).unwrap();
    let staged = root.join("candidate");
    symlink(&root, &staged).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("verify-staged")
        .arg("--json")
        .arg(&archive)
        .arg(&staged)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
    fs::remove_file(staged).unwrap();
    fs::remove_file(archive).unwrap();
    fs::remove_dir_all(root).unwrap();
}
