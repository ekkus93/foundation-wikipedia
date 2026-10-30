//! Black-box checks for the optional file-verification byte budget.
//! A verified unsigned envelope is not publisher-authenticated or installed.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin};

fn fixture() -> Vec<u8> {
    let manifest = Manifest {
        version: 1,
        pack_id: "science".into(),
        project: "enwiki".into(),
        snapshot: "20261001".into(),
        origin: Origin::Custom {
            definition_id: "cli-budget-test".into(),
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

use std::io::Write;

#[test]
fn verify_and_inspect_file_budget_is_opt_in_and_fail_closed() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "fwpack-verify-budget-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("sample.wpack");
    let bytes = fixture();
    fs::write(&path, &bytes).unwrap();
    let binary = env!("CARGO_BIN_EXE_wiki-pack");

    for command in ["verify", "inspect"] {
        let ordinary = Command::new(binary)
            .arg(command)
            .arg(&path)
            .output()
            .unwrap();
        assert!(ordinary.status.success(), "{ordinary:?}");

        let exact = Command::new(binary)
            .arg(command)
            .arg(&path)
            .arg(bytes.len().to_string())
            .output()
            .unwrap();
        assert!(exact.status.success(), "{exact:?}");
        assert!(
            String::from_utf8_lossy(&exact.stdout).contains("Publisher signature: NOT VERIFIED")
        );

        let short = Command::new(binary)
            .arg(command)
            .arg(&path)
            .arg((bytes.len() - 1).to_string())
            .output()
            .unwrap();
        assert_eq!(short.status.code(), Some(1), "{short:?}");
        assert!(String::from_utf8_lossy(&short.stderr).contains("ResourceBudget"));

        let invalid = Command::new(binary)
            .arg(command)
            .arg(&path)
            .arg("not-a-budget")
            .output()
            .unwrap();
        assert_eq!(invalid.status.code(), Some(2), "{invalid:?}");
        let extra = Command::new(binary)
            .arg(command)
            .arg(&path)
            .arg(bytes.len().to_string())
            .arg("unexpected")
            .output()
            .unwrap();
        assert_eq!(extra.status.code(), Some(2), "{extra:?}");
    }

    // An appended byte must never be accepted even if the declared objects fit.
    let mut trailing = bytes.clone();
    trailing.push(0);
    fs::write(&path, trailing).unwrap();
    let appended = Command::new(binary)
        .args(["verify", path.to_str().unwrap(), &bytes.len().to_string()])
        .output()
        .unwrap();
    assert_eq!(appended.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&appended.stderr).contains("ResourceBudget"));

    fs::remove_dir_all(directory).unwrap();
}
