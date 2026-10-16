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
    assert!(String::from_utf8_lossy(&result.stderr).contains("MissingSuppliedStylesheet"));
}

#[test]
fn css_graph_cli_rejects_directory_input() {
    let path = fixture_path();
    fs::create_dir(&path).unwrap();
    let result = execute(&path);
    fs::remove_dir(&path).unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("regular file"));
}

#[test]
fn css_graph_cli_rejects_oversized_and_malformed_input() {
    let path = fixture_path();
    let file = fs::File::create(&path).unwrap();
    file.set_len(4 * 1024 * 1024 + 1).unwrap();
    drop(file);
    let oversized = execute(&path);
    assert!(!oversized.status.success());
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("size limit"));
    fs::write(&path, b"{not-json").unwrap();
    let malformed = execute(&path);
    fs::remove_file(&path).unwrap();
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("invalid CSS graph JSON"));
}

#[test]
fn css_graph_digest_identifies_exact_supplied_bytes() {
    let path = fixture_path();
    let value = json!({
        "roots": ["https://cdn.test/root.css"],
        "stylesheets": {
            "https://cdn.test/root.css": ".figure { background: url(figure.svg) }"
        }
    });
    let original = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &original).unwrap();
    let first = execute(&path);
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let first_report: Value = serde_json::from_slice(&first.stdout).unwrap();
    let first_digest = first_report["input_sha256"].as_str().unwrap();
    assert_eq!(first_digest.len(), 64);
    assert!(first_digest.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let mut changed = original;
    changed.push(b'\\n');
    fs::write(&path, changed).unwrap();
    let second = execute(&path);
    fs::remove_file(&path).unwrap();
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    let second_report: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(first_digest, second_report["input_sha256"].as_str().unwrap());
    assert_eq!(first_report["candidates"], second_report["candidates"]);
    assert_eq!(first_report["required_media_verified"], false);
    assert_eq!(second_report["required_media_verified"], false);
}
