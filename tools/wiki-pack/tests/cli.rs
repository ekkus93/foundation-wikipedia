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
