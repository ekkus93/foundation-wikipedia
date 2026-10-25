//! Black-box CLI contract for fail-closed machine-readable source receipts.
//! These fixtures prove local integrity only, not Wikimedia provenance.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn private_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "wiki-pack-generation-json-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    root
}

fn run(root: &Path, pin: &str, json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wiki-pack"));
    command.arg("verify-public-members");
    if json {
        command.arg("--json");
    }
    command
        .arg("enwiki")
        .arg("2026-10-01")
        .arg(pin)
        .arg(root)
        .arg("1024")
        .output()
        .unwrap()
}

#[test]
fn cli_json_receipt_requires_all_pinned_members_and_preserves_human_output() {
    let root = private_root();
    let name = "enwiki-2026-10-01-p1p100.xml.bz2";
    let body = b"fixture compressed member";
    let inventory = format!("{:x}  {name}\n", Sha256::digest(body));
    let pin = format!("{:x}", Sha256::digest(inventory.as_bytes()));
    fs::write(root.join("SHA256SUMS"), inventory).unwrap();

    // No successful receipt may escape when a declared member is absent.
    let missing = run(&root, &pin, true);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());

    fs::write(root.join(name), body).unwrap();
    let good = run(&root, &pin, true);
    assert!(
        good.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&good.stderr)
    );
    let stdout = String::from_utf8(good.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["schema_version"], 2);
    assert_eq!(json["member_count"], 1);
    assert_eq!(json["verified_compressed_bytes"], body.len() as u64);
    assert_eq!(json["inventory_sha256"], pin);
    assert_eq!(json["verified_members"].as_array().unwrap().len(), 1);
    assert_eq!(json["verified_members"][0]["index"], 0);
    assert_eq!(json["verified_members"][0]["relative_path"], name);
    assert_eq!(
        json["verified_members"][0]["sha256"],
        format!("{:x}", Sha256::digest(body))
    );
    assert_eq!(
        json["verified_members"][0]["verified_compressed_bytes"],
        body.len() as u64
    );
    assert_eq!(json["trust"]["publisher_authenticated"], false);
    assert_eq!(json["trust"]["mandatory_offline_media_complete"], false);
    assert_eq!(json["trust"]["pack_installed"], false);

    let human = run(&root, &pin, false);
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("Verified 1 locally staged compressed members"));
    assert!(text.contains("Publisher provenance, independent census and rendering: NOT VERIFIED"));

    let bad_pin = run(&root, &"0".repeat(64), true);
    assert!(!bad_pin.status.success());
    assert!(bad_pin.stdout.is_empty());

    fs::write(root.join(name), b"tampered").unwrap();
    let corrupt = run(&root, &pin, true);
    assert!(!corrupt.status.success());
    assert!(corrupt.stdout.is_empty());
    fs::remove_dir_all(root).unwrap();
}
