//! Black-box CLI qualification for bounded nonseekable unsigned pack staging.
//! No publisher trust or installed-pack activation is implied.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};
use wiki_pack_builder::wiki_pack_format::sha256_hex;

fn private_parent() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("wiki-cli-stdin-{}-{stamp}", std::process::id()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root).unwrap();
    root
}

fn invoke(parent: &Path, name: &str, budget: u64, bytes: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg("stage-unsigned-stdin")
        .arg(parent)
        .arg(name)
        .arg(budget.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdin_staging_checks_integrity_and_quota_without_spool_residue() {
    let parent = private_parent();
    let payload = b"offline image bytes";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "figures".into(),
        project: "enwiki".into(),
        snapshot: "20261029".into(),
        origin: Origin::Custom {
            definition_id: "figures".into(),
        },
        objects: vec![Object {
            path: "media/figure.svg".into(),
            sha256: sha256_hex(payload),
            bytes: payload.len() as u64,
        }],
    };
    let mut encoded = Vec::new();
    write_container(&mut encoded, &manifest, |_, out| out.write_all(payload)).unwrap();
    let quota = encoded.len() as u64;

    let success = invoke(&parent, "verified", quota, &encoded);
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    let stdout = String::from_utf8_lossy(&success.stdout);
    assert!(stdout.contains("UNTRUSTED STAGING ONLY"));
    assert!(stdout.contains("Publisher signature: NOT VERIFIED"));
    assert_eq!(
        fs::read(parent.join("verified/media/figure.svg")).unwrap(),
        payload
    );

    let limited = invoke(&parent, "limited", quota - 1, &encoded);
    assert!(!limited.status.success());
    assert!(String::from_utf8_lossy(&limited.stderr).contains("ResourceBudget"));
    assert!(!parent.join("limited").exists());

    let mut corrupted = encoded;
    *corrupted.last_mut().unwrap() ^= 1;
    let failed = invoke(&parent, "corrupt", quota, &corrupted);
    assert!(!failed.status.success());
    assert!(!parent.join("corrupt").exists());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}
