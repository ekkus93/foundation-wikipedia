//! Audit receipts are exact-byte identities, not publisher authentication.
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn audit_cli_reports_consumed_record_and_expected_census_digests() {
    let root = std::env::temp_dir().join(format!(
        "fw-audit-receipt-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let revisions = root.join("records.jsonl");
    let census = root.join("expected.ids");
    let text = "Offline article";
    let record = serde_json::json!({
        "schema_version": 1,
        "project": "enwiki",
        "generation_id": "2026-10-01",
        "page_id": 7,
        "namespace": 0,
        "title": "Example",
        "revision_id": 91,
        "timestamp": "2026-09-30T12:00:00Z",
        "redirect_target": null,
        "wikitext": text,
        "wikitext_sha256": format!("{:x}", Sha256::digest(text.as_bytes()))
    });
    let content = format!("{record}\n");
    fs::write(&revisions, &content).unwrap();
    fs::write(&census, b"7\n").unwrap();
    let census_pin = format!("{:x}", Sha256::digest(b"7\n"));
    let revision_pin = format!("{:x}", Sha256::digest(content.as_bytes()));
    let invoke = |expected_pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .arg("audit-diagnostic")
            .arg(&revisions)
            .arg(&census)
            .args(["enwiki", "2026-10-01", "0", expected_pin, &revision_pin])
            .output()
            .unwrap()
    };
    let success = invoke(&census_pin);
    assert!(success.status.success(), "{success:?}");
    let stdout = String::from_utf8(success.stdout).unwrap();
    assert!(stdout.contains(&format!(
        "Expected census SHA-256 (caller pin): {census_pin}"
    )));
    assert!(stdout.contains(&format!("Revision-content SHA-256 (consumed): {revision_pin}")));
    assert!(stdout.contains("NOT AUTHENTICATED"));
    let failed = invoke(&"0".repeat(64));
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    fs::remove_dir_all(root).unwrap();
}
