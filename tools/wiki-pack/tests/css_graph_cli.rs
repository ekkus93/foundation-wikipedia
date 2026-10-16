//! Process-level tests for offline caller-supplied CSS diagnostics.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "fw-css-graph-cli-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn execute(path: &PathBuf) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("inventory-css-graph")
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn supplied_css_import_chain_is_a_diagnostic_not_verified_media() {
    let root = "https://cdn.test/css/root.css";
    let child = "https://cdn.test/css/child.css";
    let path = fixture_path();
    let value = json!({
        "roots": [root],
        "stylesheets": {
            root: "@import \"child.css\"; .x { background:url(../img/hero.png) }",
            child: "@font-face { src:url(font.woff2) }"
        }
    });
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let result = execute(&path);
    fs::remove_file(path).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["diagnostic_only"], true);
    assert_eq!(report["stylesheets_inspected"], 2);
    assert_eq!(report["observed_references"], 3);
    assert_eq!(report["unique_candidate_count"], 3);
    assert_eq!(report["required_media_verified"], false);
    assert_eq!(report["css_and_font_dependencies_verified"], false);
}

#[test]
fn css_graph_cli_rejects_unknown_import_and_bad_args() {
    let missing_args = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("inventory-css-graph")
        .output()
        .unwrap();
    assert_eq!(missing_args.status.code(), Some(2));
    let path = fixture_path();
    let value = json!({
        "roots": ["https://cdn.test/main.css"],
        "stylesheets": {
            "https://cdn.test/main.css": "@import 'absent.css';"
        }
    });
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let result = execute(&path);
    fs::remove_file(path).unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("MissingSuppliedStylesheet")
    );
}
