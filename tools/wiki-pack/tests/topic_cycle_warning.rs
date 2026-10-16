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

#[test]
fn administrative_and_depth_limited_edges_do_not_trigger_false_cycle_warnings() {
    let path = std::env::temp_dir().join(format!(
        "fw-topic-cycle-boundaries-{}.json",
        std::process::id()
    ));
    let mut input = serde_json::json!({
        "snapshot_id": "snapshot-1",
        "roots": ["Root"],
        "depth_limit": 2,
        "page_limit": 10,
        "categories": [
            {"name": "Root", "articles": [1], "children": ["Admin", "Deep"]},
            {"name": "Admin", "articles": [999], "children": ["Root"], "administrative": true},
            {"name": "Deep", "articles": [2], "children": ["Tail"]},
            {"name": "Tail", "articles": [3], "children": ["Deep"]}
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
    let limited = preview(&input);
    input["depth_limit"] = serde_json::json!(3);
    let expanded = preview(&input);
    fs::remove_file(path).unwrap();
    assert_eq!(limited["selected_page_ids"], serde_json::json!([1, 2, 3]));
    assert_eq!(expanded["selected_page_ids"], limited["selected_page_ids"]);
    assert_eq!(limited["skipped_administrative_count"], 1);
    assert!(!limited["warnings"].as_array().unwrap().iter().any(|w| {
        w.as_str()
            .is_some_and(|s| s.starts_with("Category cycle detected:"))
    }));
    assert!(expanded["warnings"].as_array().unwrap().iter().any(|w| {
        w.as_str()
            .is_some_and(|s| s.starts_with("Category cycle detected:"))
    }));
}

#[test]
fn topic_preview_json_reports_multiple_independent_cycles() {
    let path =
        std::env::temp_dir().join(format!("fw-topic-multi-cycle-{}.json", std::process::id()));
    let input = serde_json::json!({
        "snapshot_id": "snapshot-1",
        "roots": ["Root"],
        "depth_limit": 3,
        "page_limit": 10,
        "categories": [
            {"name": "Root", "articles": [1], "children": ["A", "C"]},
            {"name": "A", "articles": [2], "children": ["B"]},
            {"name": "B", "children": ["A"]},
            {"name": "C", "articles": [3], "children": ["D"]},
            {"name": "D", "children": ["C"]}
        ]
    });
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args(["preview-topic", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["selected_page_ids"], serde_json::json!([1, 2, 3]));
    let cycles: Vec<&str> = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter(|warning| warning.starts_with("Category cycle detected:"))
        .collect();
    assert_eq!(
        cycles,
        vec![
            "Category cycle detected: B -> A",
            "Category cycle detected: D -> C"
        ]
    );
}
