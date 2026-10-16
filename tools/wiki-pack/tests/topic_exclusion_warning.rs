use std::{fs, process::Command};

#[test]
fn preview_reports_suppressed_explicit_includes() {
    let path = std::env::temp_dir().join(format!("fw-conflict-{}.json", std::process::id()));
    fs::write(&path, r#"{"snapshot_id":"snapshot-1","roots":["Root"],"include":[20,30],"exclude":[2,30],"depth_limit":0,"page_limit":10,"categories":[{"name":"Root","articles":[1]}],"redirects":[{"from":20,"to":2}]}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args(["preview-topic", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["selected_page_ids"], serde_json::json!([1]));
    let warnings = result["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str() == Some(
        "Explicit include suppressed by exclusion: 20 -> 2"
    )));
    assert!(warnings.iter().any(|w| w.as_str() == Some(
        "Explicit include suppressed by exclusion: 30 -> 30"
    )));
}
