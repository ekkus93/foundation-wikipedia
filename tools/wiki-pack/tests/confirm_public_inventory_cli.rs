#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn live_confirmation_cli_is_read_only_and_emits_no_receipt_on_failure() {
    let base = std::env::temp_dir().join(format!(
        "wiki-pack-live-confirmation-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let root = base.join("source");
    let bin = base.join("bin");
    fs::create_dir(&base).unwrap();
    fs::create_dir(&root).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

    let name = "enwiki-2026-10-01-p1p100.xml.bz2";
    let inventory = format!("{}  {name}\n", "a".repeat(64));
    let pin = format!("{:x}", Sha256::digest(inventory.as_bytes()));
    let local = root.join("SHA256SUMS");
    fs::write(&local, &inventory).unwrap();
    let curl = bin.join("curl");
    let set_curl = |script: &str| {
        fs::write(&curl, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).unwrap();
    };
    let command = |digest: &str, json: bool| {
        let mut args = vec!["confirm-public-inventory"];
        if json {
            args.push("--json");
        }
        Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .args(args)
            .arg("enwiki")
            .arg("2026-10-01")
            .arg(digest)
            .arg(&root)
            .env("PATH", &bin)
            .output()
            .unwrap()
    };

    set_curl(&format!(
        "printf '%s' '{}'; printf 'FW_DUMP_HTTP_CODE:200\\n' >&2",
        inventory
    ));
    let good = command(&pin, true);
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&good.stdout).unwrap();
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["kind"], "public_dump_live_https_inventory_confirmation");
    assert_eq!(receipt["inventory_sha256"], pin);
    assert_eq!(receipt["member_count"], 1);
    assert_eq!(receipt["checks"]["official_https_inventory_retrieved"], true);
    assert_eq!(receipt["trust"]["publisher_signature_verified"], false);
    assert_eq!(receipt["trust"]["independent_page_census_verified"], false);
    assert_eq!(good.stdout.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert_eq!(fs::read(&local).unwrap(), inventory.as_bytes());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

    let human = command(&pin, false);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("NOT VERIFIED"));

    set_curl("printf 'forged'; printf 'FW_DUMP_HTTP_CODE:200\\n' >&2");
    let mismatch = command(&pin, true);
    assert!(!mismatch.status.success());
    assert!(mismatch.stdout.is_empty());

    set_curl("printf 'missing'; printf 'FW_DUMP_HTTP_CODE:404\\n' >&2");
    let unavailable = command(&pin, true);
    assert!(!unavailable.status.success());
    assert!(unavailable.stdout.is_empty());

    let wrong_pin = command(&"0".repeat(64), true);
    assert!(!wrong_pin.status.success());
    assert!(wrong_pin.stdout.is_empty());

    fs::write(&local, b"tampered").unwrap();
    let corrupt = command(&pin, true);
    assert!(!corrupt.status.success());
    assert!(corrupt.stdout.is_empty());
    assert_eq!(fs::read(&local).unwrap(), b"tampered");

    fs::remove_dir_all(base).unwrap();
}
