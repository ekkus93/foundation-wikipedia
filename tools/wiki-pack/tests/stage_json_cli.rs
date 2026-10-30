//! Black-box staging receipts: verified unsigned archives stay untrusted.
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
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

fn private_parent() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent =
        std::env::temp_dir().join(format!("fwpack-json-stage-{}-{unique}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    }
    parent
}

fn assert_untrusted_receipt(json: &serde_json::Value, staged: &std::path::Path) {
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["status"], "untrusted_staging");
    assert_eq!(json["staging_path"], staged.to_string_lossy().as_ref());
    assert_eq!(json["integrity_verified"], true);
    assert_eq!(json["publisher_signature_verified"], false);
    assert_eq!(json["offline_media_complete"], false);
    assert_eq!(json["installed"], false);
    assert_eq!(json["pack_id"], "science");
    assert_eq!(json["project"], "enwiki");
    assert_eq!(json["snapshot"], "20261030");
    assert_eq!(json["object_count"], 1);
    assert_eq!(json["declared_payload_bytes"], 7);
    assert_eq!(json["claimed_origin"], "official");
    assert_eq!(json["claimed_origin_identity"], "unverified-publisher");
    assert_eq!(
        fs::read(staged.join("articles/1.cbor")).unwrap(),
        b"article"
    );
}

#[test]
fn file_and_piped_staging_emit_untrusted_receipts_only_after_success() {
    let parent = private_parent();
    let bytes = fixture();
    let archive = parent.with_extension("wpack");
    fs::write(&archive, &bytes).unwrap();
    let binary = env!("CARGO_BIN_EXE_wiki-pack");

    let file = Command::new(binary)
        .arg("stage-unsigned")
        .arg("--json")
        .arg(&archive)
        .arg(&parent)
        .arg("file-stage")
        .arg(bytes.len().to_string())
        .output()
        .unwrap();
    assert!(file.status.success(), "{file:?}");
    let mut file_json: serde_json::Value = serde_json::from_slice(&file.stdout).unwrap();
    assert_untrusted_receipt(&file_json, &parent.join("file-stage"));

    let mut child = Command::new(binary)
        .arg("stage-unsigned-stdin")
        .arg("--json")
        .arg(&parent)
        .arg("stream-stage")
        .arg(bytes.len().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let piped = child.wait_with_output().unwrap();
    assert!(piped.status.success(), "{piped:?}");
    let stream_json: serde_json::Value = serde_json::from_slice(&piped.stdout).unwrap();
    assert_untrusted_receipt(&stream_json, &parent.join("stream-stage"));
    file_json["staging_path"] = stream_json["staging_path"].clone();
    assert_eq!(file_json, stream_json);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 2);

    let short = Command::new(binary)
        .arg("stage-unsigned")
        .arg("--json")
        .arg(&archive)
        .arg(&parent)
        .arg("rejected")
        .arg((bytes.len() - 1).to_string())
        .output()
        .unwrap();
    assert_eq!(short.status.code(), Some(1), "{short:?}");
    assert!(short.stdout.is_empty());
    assert!(!parent.join("rejected").exists());

    let mut child = Command::new(binary)
        .arg("stage-unsigned-stdin")
        .arg("--json")
        .arg(&parent)
        .arg("rejected-stream")
        .arg((bytes.len() - 1).to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&bytes).unwrap();
    let short_stream = child.wait_with_output().unwrap();
    assert_eq!(short_stream.status.code(), Some(1), "{short_stream:?}");
    assert!(short_stream.stdout.is_empty());
    assert!(!parent.join("rejected-stream").exists());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 2);

    let occupied = Command::new(binary)
        .arg("stage-unsigned")
        .arg("--json")
        .arg(&archive)
        .arg(&parent)
        .arg("file-stage")
        .output()
        .unwrap();
    assert_eq!(occupied.status.code(), Some(1));
    assert!(occupied.stdout.is_empty());

    let human = Command::new(binary)
        .arg("stage-unsigned")
        .arg(&archive)
        .arg(&parent)
        .arg("human-stage")
        .output()
        .unwrap();
    assert!(human.status.success(), "{human:?}");
    assert!(String::from_utf8_lossy(&human.stdout).contains("UNTRUSTED STAGING ONLY"));
    assert!(String::from_utf8_lossy(&human.stdout).contains("claim NOT authenticated"));

    fs::remove_file(archive).unwrap();
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn malformed_staging_invocations_exit_with_usage_status() {
    let binary = env!("CARGO_BIN_EXE_wiki-pack");
    for args in [
        vec!["stage-unsigned", "--json"],
        vec!["stage-unsigned-stdin", "--json", "parent", "name"],
        vec![
            "stage-unsigned",
            "--json",
            "source",
            "parent",
            "name",
            "bad-budget",
        ],
    ] {
        let output = Command::new(binary).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(!output.stderr.is_empty(), "{output:?}");
    }
}

#[cfg(unix)]
#[test]
fn json_staging_rejects_non_utf8_destination_before_consuming_or_publishing() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    let root = private_parent();
    let parent = root.join(OsString::from_vec(vec![b'b', b'a', b'd', 0xff]));
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    let archive = root.join("input.wpack");
    fs::write(&archive, fixture()).unwrap();
    let binary = env!("CARGO_BIN_EXE_wiki-pack");

    let file = Command::new(binary)
        .arg("stage-unsigned")
        .arg("--json")
        .arg(&archive)
        .arg(&parent)
        .arg("candidate")
        .output()
        .unwrap();
    assert_eq!(file.status.code(), Some(1), "{file:?}");
    assert!(file.stdout.is_empty());
    assert!(String::from_utf8_lossy(&file.stderr).contains("must be UTF-8"));

    // No stdin bytes are needed: destination identity is rejected first.
    let stream = Command::new(binary)
        .arg("stage-unsigned-stdin")
        .arg("--json")
        .arg(&parent)
        .arg("candidate")
        .arg("1048576")
        .output()
        .unwrap();
    assert_eq!(stream.status.code(), Some(1), "{stream:?}");
    assert!(stream.stdout.is_empty());
    assert!(String::from_utf8_lossy(&stream.stderr).contains("must be UTF-8"));
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);

    fs::remove_dir_all(root).unwrap();
}
