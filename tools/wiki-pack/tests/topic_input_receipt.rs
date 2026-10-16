//! The text-mode topic preview reports exact input bytes without changing
//! the canonical JSON selection receipt or asserting upstream authenticity.
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn text_preview_reports_exact_input_digest_and_stable_selection() {
    let path = std::env::temp_dir().join(format!(
        "fw-topic-input-receipt-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let raw = r#"{"snapshot_id":"enwiki-20261001","roots":["Mathematics"],"depth_limit":0,"page_limit":10,"categories":[{"name":"Mathematics","articles":[2,1]}]}"#;
    let invoke = |input: &str| {
        fs::write(&path, input).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .arg("preview-topic")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "preview rejected fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = String::from_utf8(output.stdout).unwrap();
        assert!(report.contains(&format!(
            "Input SHA-256 (untrusted local JSON): {:x}",
            Sha256::digest(input.as_bytes())
        )));
        assert!(report.contains("Selected page IDs: 2"));
        assert!(report.contains("source publication/authenticity: NOT VERIFIED"));
        report
    };
    let first = invoke(raw);
    let second = invoke(&format!("{raw}\n"));
    fs::remove_file(path).unwrap();
    assert_ne!(first, second);
    for label in [
        "Definition SHA-256:",
        "Graph SHA-256:",
        "Selection SHA-256:",
    ] {
        let digest_line = |report: &str| {
            report
                .lines()
                .find(|line| line.starts_with(label))
                .unwrap()
                .to_owned()
        };
        assert_eq!(digest_line(&first), digest_line(&second));
    }
}
