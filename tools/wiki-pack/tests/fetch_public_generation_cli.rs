#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn full_generation_cli_json_is_offline_repeatable_and_fails_closed() {
    let root = std::env::temp_dir().join(format!(
        "wiki-pack-generation-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let first = "enwiki-2026-10-01-p1p100.xml.bz2";
    let second = "enwiki-2026-10-01-p101p200.xml.bz2";
    let inventory = format!(
        "{:x}  {second}\n{:x}  {first}\n",
        Sha256::digest(b"second"),
        Sha256::digest(b"first"),
    );
    let pin = format!("{:x}", Sha256::digest(inventory.as_bytes()));
    fs::write(root.join("SHA256SUMS"), inventory).unwrap();
    fs::write(root.join(first), b"first").unwrap();
    fs::write(root.join(second), b"second").unwrap();

    let command = |pin: &str, json: bool| {
        let mut args = vec!["fetch-public-generation"];
        if json {
            args.push("--json");
        }
        Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .args(args)
            .arg("enwiki")
            .arg("2026-10-01")
            .arg(pin)
            .arg(&root)
            .arg("1024")
            // An already staged generation must never need curl.
            .env("PATH", "/definitely-no-curl-available")
            .output()
            .unwrap()
    };

    let output = command(&pin, true);
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["member_count"], 2);
    assert_eq!(value["verified_members"][0]["relative_path"], first);
    assert_eq!(value["verified_members"][1]["relative_path"], second);
    assert_eq!(value["trust"]["publisher_authenticated"], false);
    assert_eq!(command(&pin, true).stdout, output.stdout);

    let human = command(&pin, false);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("NOT VERIFIED"));

    let wrong_pin = command(&"0".repeat(64), true);
    assert!(!wrong_pin.status.success());
    assert!(wrong_pin.stdout.is_empty());

    fs::write(root.join("unlisted.part"), b"orphan").unwrap();
    let rejected = command(&pin, true);
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert_eq!(fs::read(root.join("unlisted.part")).unwrap(), b"orphan");

    fs::remove_dir_all(root).unwrap();
}
