use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_INPUT: AtomicU64 = AtomicU64::new(0);

fn run(input: &serde_json::Value) -> (bool, serde_json::Value, String) {
    let path = std::env::temp_dir().join(format!(
        "fw-related-cli-{}-{}.json",
        std::process::id(),
        NEXT_INPUT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, serde_json::to_vec(input).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wiki-related-preview"))
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    let json = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), json, stderr)
}

#[test]
fn cli_expands_only_bounded_nonexcluded_pages() {
    let input = serde_json::json!({
        "seeds": [1, 1],
        "exclude": [4],
        "max_hops": 2,
        "max_new_pages": 3,
        "max_edge_visits": 10,
        "links": [
            {"from": 1, "to": 2},
            {"from": 2, "to": 3},
            {"from": 3, "to": 1},
            {"from": 3, "to": 4}
        ]
    });
    let (ok, report, stderr) = run(&input);
    assert!(ok, "{stderr}");
    assert_eq!(report["selected_page_ids"], serde_json::json!([1, 2, 3]));
    assert_eq!(report["additional_page_count"], 2);
    assert_eq!(report["source_authenticated"], false);
    assert_eq!(report["offline_ready"], false);
}

#[test]
fn cli_rejects_expansion_beyond_page_budget() {
    let input = serde_json::json!({
        "seeds": [1],
        "max_hops": 1,
        "max_new_pages": 1,
        "max_edge_visits": 10,
        "links": [{"from": 1, "to": 2}, {"from": 1, "to": 3}]
    });
    let (ok, _, stderr) = run(&input);
    assert!(!ok);
    assert!(stderr.contains("TooManyPages"), "{stderr}");
}
