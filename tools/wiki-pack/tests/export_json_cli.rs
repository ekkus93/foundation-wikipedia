//! Black-box JSON export receipts must never imply publisher authentication.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn sandbox() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent =
        std::env::temp_dir().join(format!("fwpack-export-json-{}-{stamp}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    parent
}

fn run_export(
    source: &std::path::Path,
    destination: &std::path::Path,
    json: bool,
) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wiki-pack"));
    cmd.arg("export-custom");
    if json {
        cmd.arg("--json");
    }
    cmd.arg(source)
        .args(["science", "enwiki", "20261030", "user-definition"])
        .arg(destination)
        .output()
        .unwrap()
}

#[test]
fn json_export_is_durable_integrity_only_and_does_not_overwrite() {
    let parent = sandbox();
    let source = parent.join("objects");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("media")).unwrap();
    fs::write(source.join("article.cbor"), b"article").unwrap();
    fs::write(source.join("media/diagram.svg"), b"<svg/>").unwrap();

    let output = parent.join("science.wpack");
    let result = run_export(&source, &output, true);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stderr.is_empty(), "{result:?}");
    let receipt: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["status"], "untrusted_export");
    assert_eq!(receipt["output_path"], output.to_str().unwrap());
    assert_eq!(receipt["integrity_verified"], true);
    assert_eq!(receipt["publisher_signature_verified"], false);
    assert_eq!(receipt["offline_media_complete"], false);
    assert_eq!(receipt["installed"], false);
    assert_eq!(receipt["pack_id"], "science");
    assert_eq!(receipt["project"], "enwiki");
    assert_eq!(receipt["snapshot"], "20261030");
    assert_eq!(receipt["object_count"], 2);
    assert_eq!(receipt["declared_payload_bytes"], 13);
    assert_eq!(receipt["claimed_origin"], "custom");
    assert_eq!(receipt["claimed_origin_identity"], "user-definition");
    assert!(output.is_file());

    let verify = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args(["inspect", "--json"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(verify.status.success(), "{verify:?}");
    let inspected: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(inspected["integrity_verified"], true);
    assert_eq!(inspected["claimed_origin"], "custom");
    assert_eq!(inspected["publisher_signature_verified"], false);

    let bytes = fs::read(&output).unwrap();
    let collision = run_export(&source, &output, true);
    assert_eq!(collision.status.code(), Some(1), "{collision:?}");
    assert!(collision.stdout.is_empty());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 2);

    let human = run_export(&source, &parent.join("human.wpack"), false);
    assert!(human.status.success(), "{human:?}");
    assert!(String::from_utf8_lossy(&human.stdout).contains("NOT VERIFIED"));
    assert!(String::from_utf8_lossy(&human.stdout).contains("NOT installed"));

    let malformed = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args(["export-custom", "--json"])
        .output()
        .unwrap();
    assert_eq!(malformed.status.code(), Some(2));
    assert!(malformed.stdout.is_empty());
    fs::remove_dir_all(parent).unwrap();
}

#[cfg(unix)]
#[test]
fn json_export_rejects_non_utf8_output_path_before_writing() {
    use std::os::unix::ffi::OsStringExt;
    let parent = sandbox();
    let source = parent.join("objects");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("article.cbor"), b"article").unwrap();
    let bad_name = std::ffi::OsString::from_vec(b"bad-\xff.wpack".to_vec());
    let output = parent.join(bad_name);
    let rejected = run_export(&source, &output, true);
    assert_eq!(rejected.status.code(), Some(1), "{rejected:?}");
    assert!(rejected.stdout.is_empty());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}
