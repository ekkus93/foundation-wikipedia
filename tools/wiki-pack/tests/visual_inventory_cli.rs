//! Black-box qualification of the provisional visual-candidate CLI.
//! This only verifies diagnostic behavior, never installed offline media.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "fw-visual-cli-integration-{}-{}.html",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
}

#[test]
fn inventory_visuals_process_reports_candidates_without_claiming_verified_media() {
    let path = fixture_path();
    fs::write(&path, "<img src='a.png'><img src='a.png'>").unwrap();
    let output = command()
        .arg("inventory-visuals")
        .arg(&path)
        .arg("https://en.wikipedia.org/wiki/Test")
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["diagnostic_only"], true);
    assert_eq!(json["required_media_verified"], false);
    assert_eq!(json["css_and_font_dependencies_verified"], false);
    assert_eq!(json["observed_references"], 2);
    assert_eq!(json["unique_candidate_count"], 1);
    assert_eq!(
        json["inventory_sha256"],
        "03dd9d35ed55fa2c181b3d76f9c583fca66cbc8bc3c1cb03a43456120130ad3d"
    );
    assert_eq!(
        json["candidates"][0]["url"],
        "https://en.wikipedia.org/wiki/a.png"
    );
}

#[test]
fn inventory_visuals_process_resolves_html_base_and_rejects_ambiguous_base() {
    let path = fixture_path();
    fs::write(
        &path,
        "<base href='https://upload.wikimedia.org/media/'><img src='a.svg'>",
    )
    .unwrap();
    let output = command()
        .arg("inventory-visuals")
        .arg(&path)
        .arg("https://en.wikipedia.org/wiki/Test")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["html_base_url"],
        "https://upload.wikimedia.org/media/"
    );
    assert_eq!(
        report["candidates"][0]["url"],
        "https://upload.wikimedia.org/media/a.svg"
    );
    assert_eq!(report["required_media_verified"], false);

    fs::write(
        &path,
        "<base href='https://a.test/'><base href='https://b.test/'><img src='a.svg'>",
    )
    .unwrap();
    let duplicate = command()
        .arg("inventory-visuals")
        .arg(&path)
        .arg("https://en.wikipedia.org/wiki/Test")
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("MultipleBaseHref"));
}

#[test]
fn inventory_visuals_process_rejects_bad_input_and_argument_count() {
    let missing = command().arg("inventory-visuals").output().unwrap();
    assert_eq!(missing.status.code(), Some(2));

    let path = fixture_path();
    fs::write(&path, [0xff, 0xfe]).unwrap();
    let output = command()
        .arg("inventory-visuals")
        .arg(&path)
        .arg("https://en.wikipedia.org/wiki/Test")
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("UTF-8"));
}
