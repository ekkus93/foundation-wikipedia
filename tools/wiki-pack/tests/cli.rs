use std::fs::{self, File};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};

fn fixture(official: bool) -> (std::path::PathBuf, std::path::PathBuf) {
    let parent = std::env::temp_dir().join(format!(
        "wiki-pack-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&parent).unwrap();
    let path = parent.join("fixture.wpack");
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "physics".into(),
        project: "enwiki".into(),
        snapshot: "20261001".into(),
        origin: if official {
            Origin::Official {
                publisher: "untrusted-claim".into(),
            }
        } else {
            Origin::Custom {
                definition_id: "fixture".into(),
            }
        },
        objects: vec![Object {
            path: "articles/1.cbor".into(),
            sha256: "84393add8c489d33569360efd1bcb5f70ed5a22408127d241c751e5ea345eb7c".into(),
            bytes: 7,
        }],
    };
    let mut output = File::create(&path).unwrap();
    write_container(&mut output, &manifest, |_object, writer| {
        writer.write_all(b"article")
    })
    .unwrap();
    (parent, path)
}

fn invoke(command: &str, args: &[&std::ffi::OsStr]) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_wiki-pack"));
    process.arg(command);
    process.args(args);
    process.output().unwrap()
}

#[test]
fn verify_and_inspect_do_not_claim_publisher_trust() {
    for official in [false, true] {
        let (parent, path) = fixture(official);
        let verified = invoke("verify", &[path.as_os_str()]);
        assert!(verified.status.success(), "{verified:?}");
        let summary = String::from_utf8(verified.stdout).unwrap();
        assert!(summary.contains("Envelope integrity verified: 1 objects, 7"));
        assert!(summary.contains("Publisher signature: NOT VERIFIED"));
        assert!(!summary.contains("Pack ID:"));
        let inspected = invoke("inspect", &[path.as_os_str()]);
        assert!(inspected.status.success(), "{inspected:?}");
        let details = String::from_utf8(inspected.stdout).unwrap();
        assert!(details.contains("Pack ID: physics"));
        assert!(details.contains("Required offline media completeness: NOT VERIFIED"));
        if official {
            assert!(details.contains("claim NOT authenticated"));
        } else {
            assert!(details.contains("Claimed origin: custom (fixture)"));
        }
        fs::remove_dir_all(parent).unwrap();
    }
}

#[test]
fn corrupt_envelope_is_not_inspected_or_reported_verified() {
    let (parent, path) = fixture(false);
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&path, bytes).unwrap();
    for command in ["verify", "inspect"] {
        let result = invoke(command, &[path.as_os_str()]);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8(result.stderr)
            .unwrap()
            .contains("integrity verification failed"));
    }
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn usage_errors_and_missing_input_fail_closed() {
    let (parent, path) = fixture(false);
    let help = invoke("--help", &[]);
    assert!(help.status.success());
    let extra = invoke("verify", &[path.as_os_str(), path.as_os_str()]);
    assert_eq!(extra.status.code(), Some(2));
    let missing = invoke("inspect", &[std::ffi::OsStr::new("no-such-file.wpack")]);
    assert_eq!(missing.status.code(), Some(1));
    let unknown = invoke("install", &[path.as_os_str()]);
    assert_eq!(unknown.status.code(), Some(2));
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn cli_export_custom_roundtrip_is_verified_and_never_overwrites() {
    let (parent, _) = fixture(false);
    let source = parent.join("custom-objects");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("media")).unwrap();
    fs::write(source.join("article.cbor"), b"verified canonical article").unwrap();
    fs::write(source.join("media/diagram.svg"), b"<svg/>").unwrap();
    let output = parent.join("custom-output.wpack");
    let args: [&std::ffi::OsStr; 6] = [
        source.as_os_str(),
        std::ffi::OsStr::new("physics"),
        std::ffi::OsStr::new("enwiki"),
        std::ffi::OsStr::new("20261001"),
        std::ffi::OsStr::new("physics-definition"),
        output.as_os_str(),
    ];
    let exported = invoke("export-custom", &args);
    assert!(exported.status.success(), "{exported:?}");
    let verified = invoke("verify", &[output.as_os_str()]);
    assert!(verified.status.success(), "{verified:?}");
    let inspected = invoke("inspect", &[output.as_os_str()]);
    assert!(inspected.status.success(), "{inspected:?}");
    assert!(String::from_utf8(inspected.stdout)
        .unwrap()
        .contains("Claimed origin: custom (physics-definition)"));
    // End-to-end CLI path: exported bytes must stage into the exact
    // original structured content, without publisher-trust elevation.
    let private = parent.join("roundtrip-private");
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&private).unwrap();
    let stage_args: [&std::ffi::OsStr; 3] = [
        output.as_os_str(),
        private.as_os_str(),
        std::ffi::OsStr::new("roundtrip"),
    ];
    let staged = invoke("stage-unsigned", &stage_args);
    assert!(staged.status.success(), "{staged:?}");
    assert!(String::from_utf8(staged.stdout)
        .unwrap()
        .contains("pack NOT INSTALLED"));
    assert_eq!(
        fs::read(private.join("roundtrip/article.cbor")).unwrap(),
        fs::read(source.join("article.cbor")).unwrap()
    );
    assert_eq!(
        fs::read(private.join("roundtrip/media/diagram.svg")).unwrap(),
        fs::read(source.join("media/diagram.svg")).unwrap()
    );
    let original = fs::read(&output).unwrap();
    let second = invoke("export-custom", &args);
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(fs::read(&output).unwrap(), original);
    assert_eq!(
        fs::read_dir(&parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".fwpack-export."))
            .count(),
        0
    );
    let inside = source.join("self.wpack");
    let forbidden: [&std::ffi::OsStr; 6] = [
        source.as_os_str(),
        std::ffi::OsStr::new("physics"),
        std::ffi::OsStr::new("enwiki"),
        std::ffi::OsStr::new("20261001"),
        std::ffi::OsStr::new("physics-definition"),
        inside.as_os_str(),
    ];
    assert_eq!(invoke("export-custom", &forbidden).status.code(), Some(1));
    assert!(!inside.exists());
    fs::remove_dir_all(parent).unwrap();
}

#[cfg(unix)]
#[test]
fn cli_export_custom_refuses_symlinked_source() {
    use std::os::unix::fs::symlink;
    let (parent, _) = fixture(false);
    let source = parent.join("custom-source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("article.cbor"), b"content").unwrap();
    symlink(&source, parent.join("linked-source")).unwrap();
    let output = parent.join("unsafe.wpack");
    let linked = parent.join("linked-source");
    let args: [&std::ffi::OsStr; 6] = [
        linked.as_os_str(),
        std::ffi::OsStr::new("physics"),
        std::ffi::OsStr::new("enwiki"),
        std::ffi::OsStr::new("20261001"),
        std::ffi::OsStr::new("physics"),
        output.as_os_str(),
    ];
    let result = invoke("export-custom", &args);
    assert_eq!(result.status.code(), Some(1));
    assert!(!output.exists());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn cli_unsigned_staging_is_private_and_never_installs_or_replaces() {
    let (parent, original) = fixture(false);
    let private = parent.join("private-stage");
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&private).unwrap();
    let args: [&std::ffi::OsStr; 3] = [
        original.as_os_str(),
        private.as_os_str(),
        std::ffi::OsStr::new("candidate"),
    ];
    let accepted = invoke("stage-unsigned", &args);
    assert!(accepted.status.success(), "{accepted:?}");
    let stdout = String::from_utf8(accepted.stdout).unwrap();
    assert!(stdout.contains("Status: UNTRUSTED STAGING ONLY; pack NOT INSTALLED"));
    assert!(stdout.contains("Publisher signature: NOT VERIFIED"));
    assert_eq!(
        fs::read(private.join("candidate/articles/1.cbor")).unwrap(),
        b"article"
    );
    let retry = invoke("stage-unsigned", &args);
    assert_eq!(retry.status.code(), Some(1));
    assert_eq!(
        fs::read(private.join("candidate/articles/1.cbor")).unwrap(),
        b"article"
    );
    let mut corrupt = fs::read(&original).unwrap();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&original, corrupt).unwrap();
    let corrupt_args: [&std::ffi::OsStr; 3] = [
        original.as_os_str(),
        private.as_os_str(),
        std::ffi::OsStr::new("invalid"),
    ];
    assert_eq!(
        invoke("stage-unsigned", &corrupt_args).status.code(),
        Some(1)
    );
    assert!(!private.join("invalid").exists());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn cli_staged_official_claim_is_never_trusted() {
    let (parent, original) = fixture(true);
    let private = parent.join("private-stage");
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&private).unwrap();
    let args: [&std::ffi::OsStr; 3] = [
        original.as_os_str(),
        private.as_os_str(),
        std::ffi::OsStr::new("claimed"),
    ];
    let result = invoke("stage-unsigned", &args);
    assert!(result.status.success(), "{result:?}");
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("claim NOT authenticated"));
    assert!(stdout.contains("pack NOT INSTALLED"));
    fs::remove_dir_all(parent).unwrap();
}
#[test]
fn cli_previews_untrusted_local_category_graph_without_building_a_pack() {
    let (parent, _) = fixture(false);
    let input = parent.join("topic.json");
    fs::write(
        &input,
        r#"{
            "snapshot_id": "enwiki-20261001",
            "roots": ["Category:Math"],
            "include": [101],
            "exclude": [3],
            "depth_limit": 1,
            "page_limit": 10,
            "categories": [
                {"name": "Category:Math", "articles": [2, 3], "children": []}
            ],
            "redirects": [{"from": 101, "to": 2}]
        }"#,
    )
    .unwrap();
    let output = invoke("preview-topic", &[input.as_os_str()]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("source publication/authenticity: NOT VERIFIED"));
    assert!(text.contains("Selected page IDs: 1"));
    assert!(text.contains("Visited categories: 1"));
    assert!(text.contains("Resolved redirect: 101 -> 2"));
    assert!(text.contains("Definition SHA-256:"));
    assert!(text.contains("Selection SHA-256:"));
    assert!(text.contains("no pack built, signed, or installed"));
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn cli_preview_rejects_duplicate_categories_and_missing_roots() {
    let (parent, _) = fixture(false);
    let input = parent.join("topic.json");
    let base = r#"{
        "snapshot_id": "enwiki-20261001",
        "roots": ["Category:Math"],
        "depth_limit": 1,
        "page_limit": 10,
        "categories": [
            {"name": "Category:Math", "articles": [2]},
            {"name": "Category:Math", "articles": [3]}
        ]
    }"#;
    fs::write(&input, base).unwrap();
    let duplicate = invoke("preview-topic", &[input.as_os_str()]);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(duplicate.stdout.is_empty());
    assert!(String::from_utf8(duplicate.stderr)
        .unwrap()
        .contains("duplicate category name"));
    fs::write(
        &input,
        base.replace(
            "Category:Math\", \"articles\": [3]",
            "Category:Other\", \"articles\": [3]",
        ),
    )
    .unwrap();
    let missing = invoke("preview-topic", &[input.as_os_str()]);
    assert_eq!(missing.status.code(), Some(0));
    fs::write(
        &input,
        base.replace(
            "Category:Math\", \"articles\": [3]",
            "Category:Other\", \"articles\": [3]",
        )
        .replace("Category:Math\"]", "Category:Missing\"]"),
    )
    .unwrap();
    let invalid = invoke("preview-topic", &[input.as_os_str()]);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8(invalid.stderr)
        .unwrap()
        .contains("topic preview rejected"));
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn cli_json_preview_is_deterministic_and_explicitly_untrusted() {
    let (parent, _) = fixture(false);
    let input = parent.join("topic.json");
    let first = r#"{
        "snapshot_id": "enwiki-20261001",
        "roots": ["Category:Math"],
        "include": [101],
        "exclude": [3],
        "depth_limit": 1,
        "page_limit": 10,
        "categories": [
            {"name": "Category:Math", "articles": [3, 2], "children": []},
            {"name": "Category:Other", "articles": [7], "children": []}
        ],
        "redirects": [{"from": 101, "to": 2}]
    }"#;
    fs::write(&input, first).unwrap();
    let args = [std::ffi::OsStr::new("--json"), input.as_os_str()];
    let output = invoke("preview-topic", &args);
    assert!(output.status.success(), "{output:?}");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["source"], "local-untrusted-category-graph");
    assert_eq!(parsed["source_authenticated"], false);
    assert_eq!(parsed["publisher_signature_verified"], false);
    assert_eq!(parsed["offline_ready"], false);
    assert_eq!(parsed["pack_installed"], false);
    assert_eq!(parsed["snapshot_label"], "enwiki-20261001");
    assert_eq!(parsed["selected_page_ids"], serde_json::json!([2]));
    assert_eq!(parsed["selected_page_count"], 1);
    assert_eq!(parsed["visited_category_count"], 1);
    assert_eq!(parsed["explicit_include_ids"], 1);
    assert_eq!(parsed["explicit_exclude_ids"], 1);
    assert_eq!(parsed["largest_category"]["name"], "Category:Math");
    assert!(parsed["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning == "Resolved redirect: 101 -> 2"));
    for key in ["definition_sha256", "graph_sha256", "selection_sha256"] {
        assert_eq!(parsed[key].as_str().unwrap().len(), 64);
    }

    // Record order and member order must not change the machine-readable
    // selection, which is suitable for an exact-receipt GUI preview.
    let reordered = first
        .replace(
            r#"{"name": "Category:Math", "articles": [3, 2], "children": []},
            {"name": "Category:Other", "articles": [7], "children": []}"#,
            r#"{"name": "Category:Other", "articles": [7], "children": []},
            {"name": "Category:Math", "articles": [2, 3], "children": []}"#,
        );
    fs::write(&input, reordered).unwrap();
    let second = invoke("preview-topic", &args);
    assert!(second.status.success(), "{second:?}");
    let again: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(parsed, again);

    fs::write(&input, first.replace("Category:Math", "Category:Missing")).unwrap();
    let invalid = invoke("preview-topic", &args);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    let usage = invoke(
        "preview-topic",
        &[std::ffi::OsStr::new("--json"), input.as_os_str(), input.as_os_str()],
    );
    assert_eq!(usage.status.code(), Some(2));
    fs::remove_dir_all(parent).unwrap();
}
