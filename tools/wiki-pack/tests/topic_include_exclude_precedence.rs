//! Canonical exclusions win over explicit includes, including redirects.
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn preview(include: &[u64], exclude: &[u64]) -> serde_json::Value {
    let path = std::env::temp_dir().join(format!(
        "fw-topic-exclusion-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let input = serde_json::json!({
        "snapshot_id": "enwiki-20261001",
        "roots": ["Root"],
        "include": include,
        "exclude": exclude,
        "depth_limit": 0,
        "page_limit": 10,
        "categories": [{"name": "Root", "articles": [1]}],
        "redirects": [{"from": 20, "to": 2}, {"from": 40, "to": 4}]
    });
    fs::write(&path, input.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("preview-topic")
        .arg("--json")
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn exclusions_override_explicit_includes_after_redirects() {
    let first = preview(&[20, 30, 40], &[2, 30]);
    let reordered = preview(&[40, 30, 20], &[30, 2]);
    assert_eq!(first["selected_page_ids"], serde_json::json!([1, 4]));
    assert_eq!(first["selection_sha256"], reordered["selection_sha256"]);
    assert_eq!(first["definition_sha256"], reordered["definition_sha256"]);
    assert_eq!(first["source_authenticated"], false);
    assert_eq!(first["offline_ready"], false);
}
