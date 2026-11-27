//! TRUST-003: hostile `.wpack` files through the `wiki-pack` CLI. Every
//! attack must exit with a plain failure status (no panic, no signal), must
//! not claim success, and `stage-unsigned` must leave no staged tree.
//! The install-path counterpart is `crates/wiki-ffi/tests/hostile_packs.rs`.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{
    Manifest, Object, Origin, PackMetadata, FORMAT_VERSION, TRANSCRIPT_PREFIX,
};

const BODY: &[u8] = b"article bytes";

fn manifest(path: &str) -> Manifest {
    Manifest {
        version: FORMAT_VERSION,
        pack_id: "hostile".into(),
        project: "enwiki".into(),
        snapshot: "20261001".into(),
        origin: Origin::Custom {
            definition_id: "cli-hostile".into(),
        },
        metadata: PackMetadata::synthetic_fixture(),
        objects: vec![Object {
            path: path.into(),
            sha256: format!("{:x}", Sha256::digest(BODY)),
            bytes: BODY.len() as u64,
        }],
    }
}

fn good() -> Vec<u8> {
    let mut bytes = Vec::new();
    write_container(&mut bytes, &manifest("articles/1.cbor"), |_, out| {
        out.write_all(BODY)
    })
    .unwrap();
    bytes
}

/// A hand-encoded envelope whose object is named `path`, bypassing the
/// writer's validation as a hostile tool would.
fn with_object_path(path: &str) -> Vec<u8> {
    fn field(out: &mut Vec<u8>, text: &str) {
        out.extend_from_slice(&(text.len() as u64).to_be_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    let m = manifest(path);
    let mut t = TRANSCRIPT_PREFIX.to_vec();
    t.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    for id in [&m.pack_id, &m.project, &m.snapshot] {
        field(&mut t, id);
    }
    t.push(2);
    field(&mut t, "cli-hostile");
    t.push(m.metadata.source.kind.tag());
    field(&mut t, &m.metadata.source.snapshot_id);
    field(&mut t, &m.metadata.source.manifest_sha256);
    field(&mut t, &m.metadata.definition_sha256);
    field(&mut t, &m.metadata.license.text_spdx);
    t.push(0);
    t.extend_from_slice(&0u64.to_be_bytes());
    t.extend_from_slice(&[0; 24]);
    t.extend_from_slice(&1u64.to_be_bytes());
    field(&mut t, path);
    field(&mut t, &m.objects[0].sha256);
    t.extend_from_slice(&(BODY.len() as u64).to_be_bytes());
    let mut out = b"FWPACK01".to_vec();
    out.extend_from_slice(&(t.len() as u32).to_be_bytes());
    out.extend_from_slice(&t);
    out.extend_from_slice(BODY);
    out
}

struct Lab(PathBuf);

impl Lab {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "fwpack-hostile-cli-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        fs::create_dir(dir.join("stage")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.join("stage"), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
            .args(args)
            .output()
            .unwrap()
    }

    fn assert_clean_failure(&self, attack: &str, output: &Output) {
        // A panic exits 101; a crash has no exit code at all.
        let code = output.status.code();
        assert!(
            matches!(code, Some(c) if c != 0 && c != 101),
            "{attack}: expected a plain failure, got {code:?}: {output:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("panicked"), "{attack}: {stderr}");
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("Integrity: VERIFIED"),
            "{attack}: claimed success"
        );
    }

    fn attack(&self, name: &str, pack: &[u8]) {
        let path = self.0.join(format!("{name}.wpack"));
        fs::write(&path, pack).unwrap();
        let path_text = path.to_str().unwrap();
        for command in ["verify", "inspect"] {
            let output = self.run(&[command, path_text]);
            self.assert_clean_failure(&format!("{name} {command}"), &output);
            let output = self.run(&[command, "--json", path_text]);
            self.assert_clean_failure(&format!("{name} {command} --json"), &output);
        }
        // A byte budget makes no hostile pack acceptable either.
        let output = self.run(&["verify", path_text, "1048576"]);
        self.assert_clean_failure(&format!("{name} verify budget"), &output);
        let stage = self.0.join("stage");
        let output = self.run(&[
            "stage-unsigned",
            path_text,
            stage.to_str().unwrap(),
            "victim",
        ]);
        self.assert_clean_failure(&format!("{name} stage-unsigned"), &output);
        assert!(
            fs::read_dir(&stage).unwrap().next().is_none(),
            "{name}: staging left residue"
        );
        assert_nothing_escaped(&self.0, name);
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_nothing_escaped(lab: &Path, attack: &str) {
    for entry in fs::read_dir(lab).unwrap() {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy().into_owned();
        assert!(
            name == "stage" || name.ends_with(".wpack"),
            "{attack}: unexpected entry {name}"
        );
    }
}

#[test]
fn the_control_pack_verifies() {
    let lab = Lab::new();
    let path = lab.0.join("good.wpack");
    fs::write(&path, good()).unwrap();
    let output = lab.run(&["verify", path.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn hostile_object_paths_are_refused_by_the_cli() {
    let lab = Lab::new();
    for (name, path) in [
        ("traversal", "../escape.cbor"),
        ("deep_traversal", "a/../../../escape.cbor"),
        ("absolute", "/tmp/escape.cbor"),
        ("backslash", "..\\escape.cbor"),
        ("nul_byte", "a\0b.cbor"),
        ("windows_device", "con.cbor"),
        ("padded_device", "COM1 .txt"),
        ("rtl_override", "a\u{202e}b.cbor"),
        ("decomposed_unicode", "cafe\u{301}.cbor"),
        ("drive_letter", "C:evil.cbor"),
    ] {
        lab.attack(name, &with_object_path(path));
    }
}

#[test]
fn malformed_envelopes_are_refused_by_the_cli() {
    let lab = Lab::new();
    let pack = good();
    let mut trailing = pack.clone();
    trailing.push(0);
    let mut flipped = pack.clone();
    *flipped.last_mut().unwrap() ^= 1;
    let mut huge_manifest = pack.clone();
    huge_manifest[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    let mut zero_manifest = pack.clone();
    zero_manifest[8..12].copy_from_slice(&0u32.to_be_bytes());
    for (name, bytes) in [
        ("empty", Vec::new()),
        ("magic_only", b"FWPACK01".to_vec()),
        ("truncated_mid_manifest", pack[..30].to_vec()),
        ("truncated_last_byte", pack[..pack.len() - 1].to_vec()),
        ("trailing_byte", trailing),
        ("flipped_object_byte", flipped),
        ("manifest_length_u32_max", huge_manifest),
        ("manifest_length_zero", zero_manifest),
        ("zip_renamed", b"PK\x03\x04................".to_vec()),
        ("html_renamed", b"<script>alert(1)</script>".to_vec()),
    ] {
        lab.attack(name, &bytes);
    }
}

#[test]
fn an_input_over_the_byte_budget_is_refused_by_the_cli() {
    let lab = Lab::new();
    let pack = good();
    let path = lab.0.join("budget.wpack");
    fs::write(&path, &pack).unwrap();
    let path_text = path.to_str().unwrap();
    let short = (pack.len() - 1).to_string();
    let output = lab.run(&["verify", path_text, &short]);
    lab.assert_clean_failure("budget one byte short", &output);
    let output = lab.run(&["verify", path_text, "0"]);
    lab.assert_clean_failure("budget zero", &output);
}
