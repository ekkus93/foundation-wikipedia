//! The raw local JSON receipt is distinct from canonical graph/selection hashes.
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn preview_reports_exact_input_digest_without_changing_canonical_selection() {
    let path = std::env::temp_dir().join(format!(
        "fw-topic-input-receipt-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let raw = r#"{"snapshot_id":"enwiki-20261001","roots":["Mathematics"],"depth_limit":0,"page_limit":10,"categories":[{"name":"Mathematics","articles":[2,1]}]}"#;
    let invoke = |input: &str| {
        fs::write(&path, input).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .args(["preview-topic", "--json"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "preview rejected fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report["input_sha256"],
            format!("{:x}", Sha256::digest(input.as_bytes()))
        );
        assert_eq!(report["source_authenticated"], false);
        assert_eq!(report["offline_ready"], false);
        report
    };
    let first = invoke(raw);
    let second = invoke(&format!("{raw}\n"));
    fs::remove_file(path).unwrap();
    assert_ne!(first["input_sha256"], second["input_sha256"]);
    assert_eq!(first["graph_sha256"], second["graph_sha256"]);
    assert_eq!(first["definition_sha256"], second["definition_sha256"]);
    assert_eq!(first["selection_sha256"], second["selection_sha256"]);
    assert_eq!(first["selected_page_ids"], serde_json::json!([1, 2]));
}
