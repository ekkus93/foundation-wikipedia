//! Black-box SRC-003 operator acceptance for combined local + live checks.
//! Synthetic fake-curl response; does not claim a real Wikimedia observation.
#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn private_dir() -> PathBuf {
    let parent = std::env::temp_dir().join(format!(
        "wiki-live-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    parent
}

fn curl_script(bin: &Path, script: &str) {
    let path = bin.join("curl");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn invoke(root: &Path, bin: &Path, pin: &str, json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wiki-pack"));
    command.arg("verify-public-generation-live");
    if json {
        command.arg("--json");
    }
    command
        .arg("enwiki")
        .arg("2026-10-01")
        .arg(pin)
        .arg(root)
        .arg("1024")
        .env("PATH", bin)
        .output()
        .unwrap()
}

#[test]
fn live_generation_cli_outputs_only_successful_verification_receipts() {
    let parent = private_dir();
    let root = parent.join("staged");
    let bin = parent.join("bin");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

    let first = "enwiki-2026-10-01-p1p100.xml.bz2";
    let second = "enwiki-2026-10-01-p101p200.xml.bz2";
    let raw = format!(
        "{:x}  {first}\n{:x}  {second}\n",
        Sha256::digest(b"member-one"),
        Sha256::digest(b"member-two"),
    );
    let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
    fs::write(root.join("SHA256SUMS"), &raw).unwrap();
    fs::write(root.join(first), b"member-one").unwrap();

    // A missing late shard must fail before curl can run at all.
    let marker = parent.join("transport-was-called");
    curl_script(
        &bin,
        &format!("printf invoked > '{}'; exit 1", marker.display()),
    );
    let missing = invoke(&root, &bin, &pin, true);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    assert!(!marker.exists());

    fs::write(root.join(second), b"member-two").unwrap();
    curl_script(
        &bin,
        &format!(
            "printf invoked > '{}'\nprintf '%s' '{}'\nprintf 'FW_DUMP_HTTP_CODE:200\\n' >&2",
            marker.display(),
            raw
        ),
    );
    let ok = invoke(&root, &bin, &pin, true);
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(marker.exists());
    let stdout = String::from_utf8(ok.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let receipt: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(receipt["schema_version"], 3);
    assert_eq!(
        receipt["kind"],
        "public_dump_live_https_and_local_integrity"
    );
    assert_eq!(receipt["member_count"], 2);
    assert_eq!(receipt["verified_members"][0]["relative_path"], first);
    assert_eq!(receipt["verified_members"][1]["relative_path"], second);
    assert_eq!(receipt["checks"]["member_sha256"], true);
    assert_eq!(receipt["checks"]["live_inventory_matches_local_pin"], true);
    assert_eq!(receipt["trust"]["publisher_authenticated"], false);
    assert_eq!(receipt["trust"]["independent_page_census_verified"], false);

    let human = invoke(&root, &bin, &pin, false);
    assert!(human.status.success());
    let output = String::from_utf8(human.stdout).unwrap();
    assert!(output.contains("live official HTTPS inventory: VERIFIED"));
    assert!(output.contains("Publisher signature, independent census"));

    // Corrupt bytes must fail before HTTP and must never emit a JSON receipt.
    fs::remove_file(&marker).unwrap();
    fs::write(root.join(second), b"corrupted").unwrap();
    let corrupt = invoke(&root, &bin, &pin, true);
    assert!(!corrupt.status.success());
    assert!(corrupt.stdout.is_empty());
    assert!(!marker.exists());
    fs::write(root.join(second), b"member-two").unwrap();

    // The official endpoint can be missing/changed; no local success receipt.
    curl_script(&bin, "printf 'FW_DUMP_HTTP_CODE:404\\n' >&2; exit 0");
    let unavailable = invoke(&root, &bin, &pin, true);
    assert!(!unavailable.status.success());
    assert!(unavailable.stdout.is_empty());
    let wrong_pin = invoke(&root, &bin, &"0".repeat(64), true);
    assert!(!wrong_pin.status.success());
    assert!(wrong_pin.stdout.is_empty());

    assert_eq!(fs::read(root.join(first)).unwrap(), b"member-one");
    assert_eq!(fs::read(root.join(second)).unwrap(), b"member-two");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
    fs::remove_dir_all(parent).unwrap();
}
