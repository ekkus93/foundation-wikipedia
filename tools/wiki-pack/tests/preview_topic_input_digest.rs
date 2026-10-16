//! The machine-readable topic preview must identify exact local input bytes,
//! independently of the canonical graph and resolved page-set digests.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn preview(bytes: &[u8]) -> serde_json::Value {
    let path = std::env::temp_dir().join(format!(
        "fw-topic-receipt-{}-{}.json",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("preview-topic")
        .arg("--json")
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(&path).unwrap();
    assert!(
        output.status.success(),
        "preview failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn json_preview_binds_exact_input_bytes_without_changing_canonical_selection() {
    let compact = br#"{"snapshot_id":"test-2026-10","roots":["Science"],"depth_limit":0,"page_limit":10,"categories":[{"name":"Science","articles":[42]}]}"#;
    let spaced = br#"{ "snapshot_id": "test-2026-10", "roots": ["Science"], "depth_limit": 0, "page_limit": 10, "categories": [{"name":"Science","articles":[42]}] }"#;
    let a = preview(compact);
    let b = preview(spaced);
    assert_eq!(a["schema_version"], 1);
    assert_eq!(a["source_authenticated"], false);
    assert_eq!(a["selected_page_ids"], serde_json::json!([42]));
    assert_eq!(a["input_sha256"], format!("{:x}", Sha256::digest(compact)));
    assert_eq!(b["input_sha256"], format!("{:x}", Sha256::digest(spaced)));
    assert_ne!(a["input_sha256"], b["input_sha256"]);
    assert_eq!(a["graph_sha256"], b["graph_sha256"]);
    assert_eq!(a["selection_sha256"], b["selection_sha256"]);
}
