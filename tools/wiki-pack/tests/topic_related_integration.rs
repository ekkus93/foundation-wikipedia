use std::{fs, process::Command};

fn preview(input: &serde_json::Value) -> (bool, serde_json::Value) {
    let path = std::env::temp_dir().join(format!("fw-related-topic-{}.json", std::process::id()));
    fs::write(&path, serde_json::to_vec(input).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args(["preview-topic", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    if output.status.success() {
        (true, serde_json::from_slice(&output.stdout).unwrap())
    } else {
        (
            false,
            serde_json::json!(String::from_utf8_lossy(&output.stderr).to_string()),
        )
    }
}

fn input() -> serde_json::Value {
    serde_json::json!({
        "snapshot_id": "local-snapshot",
        "roots": ["Root"],
        "page_limit": 8,
        "depth_limit": 0,
        "categories": [{"name": "Root", "articles": [1]}],
        "redirects": [{"from": 2, "to": 20}],
        "exclude": [20],
        "related": {
            "max_hops": 2,
            "max_new_pages": 3,
            "max_edge_visits": 8,
            "links": [
                {"from": 1, "to": 2},
                {"from": 1, "to": 3},
                {"from": 3, "to": 4},
                {"from": 4, "to": 1}
            ]
        }
    })
}

#[test]
fn optional_related_expansion_is_deterministic_and_exclusion_aware() {
    let mut request = input();
    let (ok, first) = preview(&request);
    assert!(ok, "{first}");
    assert_eq!(first["selected_page_ids"], serde_json::json!([1, 3, 4]));
    assert_eq!(first["selected_page_count"], 3);
    assert_eq!(first["related_added_page_count"], 2);
    assert_eq!(first["related_source_authenticated"], false);
    assert_ne!(
        first["category_selection_sha256"],
        first["selection_sha256"]
    );
    request["related"]["links"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let (ok, reordered) = preview(&request);
    assert!(ok, "{reordered}");
    assert_eq!(first["selection_sha256"], reordered["selection_sha256"]);
    assert_ne!(first["input_sha256"], reordered["input_sha256"]);
    request.as_object_mut().unwrap().remove("related");
    let (ok, base) = preview(&request);
    assert!(ok, "{base}");
    assert_eq!(base["selected_page_ids"], serde_json::json!([1]));
    assert_eq!(base["selection_sha256"], first["category_selection_sha256"]);
    assert!(base.get("related_added_page_count").is_none());
}

#[test]
fn optional_related_expansion_fails_closed_on_limits() {
    let mut request = input();
    request["related"]["max_new_pages"] = serde_json::json!(1);
    let (ok, message) = preview(&request);
    assert!(!ok);
    assert!(message.as_str().unwrap().contains("TooManyPages"));
    request["related"]["max_new_pages"] = serde_json::json!(3);
    request["page_limit"] = serde_json::json!(2);
    let (ok, message) = preview(&request);
    assert!(!ok);
    assert!(message.as_str().unwrap().contains("topic page limit"));
}
