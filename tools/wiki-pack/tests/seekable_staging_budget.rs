//! Black-box qualification of the seekable unsigned staging input quota.
//! No test here promotes an envelope to a publisher-authenticated install.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};

fn private_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn seekable_staging_cli_enforces_optional_input_budget_without_partial_output() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!(
        "wiki-cli-stage-quota-{}-{stamp}",
        std::process::id()
    ));
    private_dir(&work);
    let parent = work.join("private");
    private_dir(&parent);

    let payload = b"offline figure payload";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "sample".into(),
        project: "enwiki".into(),
        snapshot: "20261030".into(),
        origin: Origin::Custom {
            definition_id: "topic".into(),
        },
        objects: vec![Object {
            path: "media/figure.svg".into(),
            sha256: wiki_pack_builder::wiki_pack_format::sha256_hex(payload),
            bytes: payload.len() as u64,
        }],
    };
    let mut envelope = Vec::new();
    write_container(&mut envelope, &manifest, |_, output| {
        output.write_all(payload)
    })
    .unwrap();
    let input = work.join("sample.wpack");
    fs::write(&input, &envelope).unwrap();

    let binary = env!("CARGO_BIN_EXE_wiki-pack");
    let rejected = Command::new(binary)
        .arg("stage-unsigned")
        .arg(&input)
        .arg(&parent)
        .arg("over-budget")
        .arg((envelope.len() - 1).to_string())
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("ResourceBudget"));
    assert!(!parent.join("over-budget").exists());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);

    let accepted = Command::new(binary)
        .arg("stage-unsigned")
        .arg(&input)
        .arg(&parent)
        .arg("exact-budget")
        .arg(envelope.len().to_string())
        .output()
        .unwrap();
    assert!(accepted.status.success(), "{accepted:?}");
    assert!(String::from_utf8_lossy(&accepted.stdout).contains("UNTRUSTED STAGING ONLY"));
    assert_eq!(
        fs::read(parent.join("exact-budget/media/figure.svg")).unwrap(),
        payload
    );

    // The pre-existing three-argument command remains supported.
    let legacy = Command::new(binary)
        .arg("stage-unsigned")
        .arg(&input)
        .arg(&parent)
        .arg("legacy")
        .output()
        .unwrap();
    assert!(legacy.status.success(), "{legacy:?}");
    assert_eq!(
        fs::read(parent.join("legacy/media/figure.svg")).unwrap(),
        payload
    );

    let invalid = Command::new(binary)
        .arg("stage-unsigned")
        .arg(&input)
        .arg(&parent)
        .arg("invalid")
        .arg("not-a-budget")
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(!parent.join("invalid").exists());

    fs::remove_dir_all(work).unwrap();
}
