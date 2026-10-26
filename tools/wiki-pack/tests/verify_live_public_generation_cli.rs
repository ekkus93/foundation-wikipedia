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
    // HTTP 200 alone is insufficient: the official inventory must match the
    // exact pinned local publication, even when all staged shards are intact.
    let changed_inventory = raw.replacen(
        &format!("{:x}", Sha256::digest(b"member-two")),
        &"0".repeat(64),
        1,
    );
    assert_ne!(changed_inventory, raw);
    curl_script(
        &bin,
        &format!(
            "printf '%s' '{}'\\nprintf 'FW_DUMP_HTTP_CODE:200\\\\n' >&2",
            changed_inventory
        ),
    );
    let changed = invoke(&root, &bin, &pin, true);
    assert!(!changed.status.success());
    assert!(changed.stdout.is_empty());

    // A transport failure must not be converted into success by a 200 marker
    // and otherwise correct response bytes.
    curl_script(
        &bin,
        &format!(
            "printf '%s' '{}'\\nprintf 'FW_DUMP_HTTP_CODE:200\\\\n' >&2\\nexit 7",
            raw
        ),
    );
    let failed_transport = invoke(&root, &bin, &pin, true);
    assert!(!failed_transport.status.success());
    assert!(failed_transport.stdout.is_empty());

    // The subprocess must provide one canonical curl HTTP status marker.
    // Even an exact digest-pinned response body cannot redeem malformed,
    // ambiguous or non-200 diagnostics.
    for diagnostics in [
        "FW_DUMP_HTTP_CODE:200\nFW_DUMP_HTTP_CODE:200\n",
        "FW_DUMP_HTTP_CODE:200\nFW_DUMP_HTTP_CODE:404\n",
        "FW_DUMP_HTTP_CODE:+200\n",
        "FW_DUMP_HTTP_CODE:0200\n",
        "FW_DUMP_HTTP_CODE:200x\n",
        "FW_DUMP_HTTP_CODE:206\n",
        "",
    ] {
        curl_script(
            &bin,
            &format!("printf '%s' '{}'\nprintf '%s' '{}' >&2", raw, diagnostics),
        );
        let malformed = invoke(&root, &bin, &pin, true);
        assert!(
            !malformed.status.success(),
            "malformed status unexpectedly accepted: {diagnostics:?}"
        );
        assert!(malformed.stdout.is_empty());
    }

    let wrong_pin = invoke(&root, &bin, &"0".repeat(64), true);
    assert!(!wrong_pin.status.success());
    assert!(wrong_pin.stdout.is_empty());

    assert_eq!(fs::read(root.join(first)).unwrap(), b"member-one");
    assert_eq!(fs::read(root.join(second)).unwrap(), b"member-two");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn live_generation_cli_rejects_filesystem_aliases_before_transport() {
    use std::os::unix::fs::symlink;

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
        Sha256::digest(b"member-two")
    );
    let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
    fs::write(root.join("SHA256SUMS"), &raw).unwrap();
    fs::write(root.join(first), b"member-one").unwrap();
    fs::write(root.join(second), b"member-two").unwrap();

    let marker = parent.join("transport-invoked");
    curl_script(
        &bin,
        &format!("printf invoked > '{}'; exit 0", marker.display()),
    );
    let reject_before_http = |label: &str, source: &Path, digest: &str| {
        let result = invoke(source, &bin, digest, true);
        assert!(
            !result.status.success(),
            "{label} unexpectedly succeeded: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(
            result.stdout.is_empty(),
            "{label} emitted a success receipt"
        );
        assert!(!marker.exists(), "{label} reached curl");
    };

    // A private directory cannot be made trustworthy through a symlink alias.
    let root_alias = parent.join("root-alias");
    symlink(&root, &root_alias).unwrap();
    reject_before_http("symlink root", &root_alias, &pin);
    fs::remove_file(root_alias).unwrap();

    // The complete generation is untrusted if ANY member has another inode
    // alias, even when the aliased bytes have the expected SHA-256.
    let member_alias = parent.join("member-hardlink");
    fs::hard_link(root.join(first), &member_alias).unwrap();
    reject_before_http("hardlinked member", &root, &pin);
    fs::remove_file(member_alias).unwrap();

    let member_original = parent.join("member-original");
    fs::rename(root.join(first), &member_original).unwrap();
    symlink(&member_original, root.join(first)).unwrap();
    reject_before_http("symlinked member", &root, &pin);
    fs::remove_file(root.join(first)).unwrap();
    fs::rename(member_original, root.join(first)).unwrap();

    let inventory_alias = parent.join("inventory-hardlink");
    fs::hard_link(root.join("SHA256SUMS"), &inventory_alias).unwrap();
    reject_before_http("hardlinked inventory", &root, &pin);
    fs::remove_file(inventory_alias).unwrap();

    let inventory_original = parent.join("inventory-original");
    fs::rename(root.join("SHA256SUMS"), &inventory_original).unwrap();
    symlink(&inventory_original, root.join("SHA256SUMS")).unwrap();
    reject_before_http("symlinked inventory", &root, &pin);
    fs::remove_file(root.join("SHA256SUMS")).unwrap();
    fs::rename(inventory_original, root.join("SHA256SUMS")).unwrap();

    // Orphan checkpoints and undeclared directories also invalidate the
    // exact staged tree, even if every declared member remains intact.
    fs::write(root.join("orphan.part"), b"untrusted checkpoint").unwrap();
    reject_before_http("orphan checkpoint", &root, &pin);
    fs::remove_file(root.join("orphan.part")).unwrap();

    fs::create_dir(root.join("undeclared")).unwrap();
    reject_before_http("unlisted directory", &root, &pin);
    fs::remove_dir(root.join("undeclared")).unwrap();

    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    reject_before_http("nonprivate root", &root, &pin);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

    reject_before_http("incorrect independent pin", &root, &"0".repeat(64));

    // Recovery is deterministic: all rejected cases leave the staged
    // generation unchanged, and a later legitimate HTTPS check succeeds.
    assert_eq!(fs::read(root.join("SHA256SUMS")).unwrap(), raw.as_bytes());
    assert_eq!(fs::read(root.join(first)).unwrap(), b"member-one");
    assert_eq!(fs::read(root.join(second)).unwrap(), b"member-two");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
    assert!(!marker.exists());

    let curl_args = parent.join("curl-args");
    curl_script(
        &bin,
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\nprintf 'FW_DUMP_HTTP_CODE:200\\n' >&2",
            curl_args.display(),
            raw
        ),
    );
    let accepted = invoke(&root, &bin, &pin, true);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&accepted.stdout).unwrap();
    assert_eq!(receipt["member_count"], 2);
    assert_eq!(receipt["trust"]["publisher_authenticated"], false);

    // Verify the production CLI's actual transport argv, not just its
    // lower-level transport unit test: no redirects, no insecure TLS and no
    // caller-controlled origin URL.
    let args = fs::read_to_string(&curl_args).unwrap();
    let words: Vec<_> = args.lines().collect();
    assert_eq!(words.first(), Some(&"-q"));
    assert!(words.windows(2).any(|pair| pair == ["--proto", "=https"]));
    assert!(words.windows(2).any(|pair| pair == ["--max-redirs", "0"]));
    assert!(words.windows(2).any(|pair| pair == [
        "--",
        "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS"
    ]));
    assert!(!words
        .iter()
        .any(|word| ["-L", "-k", "--insecure", "--location"].contains(word)));

    fs::remove_dir_all(parent).unwrap();
}
