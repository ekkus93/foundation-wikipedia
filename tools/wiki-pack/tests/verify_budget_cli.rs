//! Black-box checks for the optional file-verification byte budget.
//! A verified unsigned envelope is not publisher-authenticated or installed.
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin};

fn fixture_with_origin(origin: Origin) -> Vec<u8> {
    let manifest = Manifest {
        version: 1,
        pack_id: "science".into(),
        project: "enwiki".into(),
        snapshot: "20261001".into(),
        origin,
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

fn fixture() -> Vec<u8> {
    fixture_with_origin(Origin::Custom {
        definition_id: "cli-budget-test".into(),
    })
}

#[test]
fn json_inspection_never_authenticates_claimed_official_origin() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "fwpack-json-claimed-origin-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("untrusted-official-claim.wpack");
    fs::write(
        &path,
        fixture_with_origin(Origin::Official {
            publisher: "unverified-publisher".into(),
        }),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("inspect")
        .arg("--json")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["claimed_origin"], "official");
    assert_eq!(json["claimed_origin_identity"], "unverified-publisher");
    assert_eq!(json["integrity_verified"], true);
    assert_eq!(json["publisher_signature_verified"], false);
    assert_eq!(json["offline_media_complete"], false);
    assert_eq!(json["installed"], false);

    fs::remove_dir_all(directory).unwrap();
}

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

        let json = Command::new(binary)
            .arg(command)
            .arg("--json")
            .arg(&path)
            .arg(bytes.len().to_string())
            .output()
            .unwrap();
        assert!(json.status.success(), "{json:?}");
        let parsed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(parsed["integrity_verified"], true);
        assert_eq!(parsed["publisher_signature_verified"], false);
        assert_eq!(parsed["offline_media_complete"], false);
        assert_eq!(parsed["installed"], false);
        assert_eq!(parsed["object_count"], 1);
        assert_eq!(parsed["declared_payload_bytes"], 7);
        if command == "inspect" {
            assert_eq!(parsed["pack_id"], "science");
            assert_eq!(parsed["project"], "enwiki");
            assert_eq!(parsed["claimed_origin"], "custom");
            assert_eq!(parsed["claimed_origin_identity"], "cli-budget-test");
        } else {
            assert!(parsed.get("pack_id").is_none());
        }

        let stdin_command = if command == "verify" {
            "verify-stdin"
        } else {
            "inspect-stdin"
        };
        let mut child = Command::new(binary)
            .arg(stdin_command)
            .arg("--json")
            .arg(bytes.len().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        let piped = child.wait_with_output().unwrap();
        assert!(piped.status.success(), "{piped:?}");
        let piped_json: serde_json::Value = serde_json::from_slice(&piped.stdout).unwrap();
        assert_eq!(piped_json, parsed);

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
