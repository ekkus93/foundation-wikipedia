//! Black-box checks for read-only, bounded stdin envelope verification.
//! No temporary archive, installed pack or publisher trust is created.

use std::io::Write;
use std::process::{Command, Stdio};

use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{Manifest, Object, Origin, FORMAT_VERSION};
use wiki_pack_builder::wiki_pack_format::sha256_hex;

fn invoke(command: &str, budget: &str, bytes: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .arg(command)
        .arg(budget)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdin_verification_checks_all_bytes_and_never_promotes_official_claim() {
    let payload = b"offline figure bytes";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "figure-pack".into(),
        project: "enwiki".into(),
        snapshot: "20261029".into(),
        origin: Origin::Official {
            publisher: "unverified-claim".into(),
        },
        objects: vec![Object {
            path: "media/figure.svg".into(),
            sha256: sha256_hex(payload),
            bytes: payload.len() as u64,
        }],
    };
    let mut bytes = Vec::new();
    write_container(&mut bytes, &manifest, |_, out| out.write_all(payload)).unwrap();
    let exact = bytes.len().to_string();

    let verified = invoke("verify-stdin", &exact, &bytes);
    assert!(verified.status.success(), "{:?}", verified.stderr);
    let text = String::from_utf8_lossy(&verified.stdout);
    assert!(text.contains("Envelope integrity verified: 1 objects"));
    assert!(text.contains("Publisher signature: NOT VERIFIED"));
    assert!(text.contains("Required offline media completeness: NOT VERIFIED"));
    assert!(!text.contains("Pack ID:"));

    let inspected = invoke("inspect-stdin", &exact, &bytes);
    assert!(inspected.status.success(), "{:?}", inspected.stderr);
    let detail = String::from_utf8_lossy(&inspected.stdout);
    assert!(detail.contains("Pack ID: figure-pack"));
    assert!(detail.contains("claim NOT authenticated"));

    let too_small = invoke("verify-stdin", &(bytes.len() - 1).to_string(), &bytes);
    assert!(!too_small.status.success());
    assert!(String::from_utf8_lossy(&too_small.stderr).contains("ResourceBudget"));

    let zero = invoke("verify-stdin", "0", &bytes);
    assert!(!zero.status.success());
    assert!(String::from_utf8_lossy(&zero.stderr).contains("ResourceBudget"));

    let mut corrupted = bytes.clone();
    *corrupted.last_mut().unwrap() ^= 1;
    let invalid = invoke("verify-stdin", &exact, &corrupted);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("ObjectDigest"));

    let mut trailing = bytes;
    trailing.push(0);
    let extra = invoke("verify-stdin", &trailing.len().to_string(), &trailing);
    assert!(!extra.status.success());
    assert!(String::from_utf8_lossy(&extra.stderr).contains("TrailingBytes"));
}

#[test]
fn stdin_verification_rejects_malformed_budgets() {
    for invalid in ["not-a-number", "-1", "18446744073709551616"] {
        let result = invoke("inspect-stdin", invalid, b"");
        assert_eq!(result.status.code(), Some(2));
    }
}
