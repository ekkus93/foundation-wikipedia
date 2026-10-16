use std::{fs, process::Command};

#[test]
fn topic_preview_json_reports_real_cycles_not_shared_descendants() {
    let path = std::env::temp_dir().join(format!("fw-topic-cycle-{}.json", std::process::id()));
    let mut input = serde_json::json!({
        "snapshot_id": "snapshot-1",
        "roots": ["Root"],
        "depth_limit": 4,
        "page_limit": 10,
        "categories": [
            {"name": "Root", "articles": [1], "children": ["A", "B"]},
            {"name": "A", "children": ["Shared"]},
            {"name": "B", "children": ["Shared"]},
            {"name": "Shared", "articles": [2]}
        ]
    });
    let preview = |input: &serde_json::Value| {
        fs::write(&path, serde_json::to_vec(input).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .args(["preview-topic", "--json"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let dag = preview(&input);
    input["categories"][3]["children"] = serde_json::json!(["Root"]);
    let cycle = preview(&input);
    fs::remove_file(path).unwrap();
    assert_eq!(dag["selected_page_ids"], serde_json::json!([1, 2]));
    assert_eq!(cycle["selected_page_ids"], dag["selected_page_ids"]);
    assert!(!dag["warnings"].as_array().unwrap().iter().any(|w| {
        w.as_str()
            .is_some_and(|s| s.starts_with("Category cycle detected:"))
    }));
    assert!(cycle["warnings"].as_array().unwrap().iter().any(|w| {
        w.as_str()
            .is_some_and(|s| s.starts_with("Category cycle detected:"))
    }));
}
