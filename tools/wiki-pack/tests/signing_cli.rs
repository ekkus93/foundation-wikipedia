//! TRUST-001: black-box checks of the publisher signing commands. Uses a
//! freshly generated throwaway key; no key is pinned in the app.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use wiki_pack_builder::wiki_pack_format::container::write_container;
use wiki_pack_builder::wiki_pack_format::manifest::{
    Manifest, Object, Origin, PackMetadata, FORMAT_VERSION,
};
use wiki_pack_builder::wiki_pack_format::sha256_hex;

fn run(args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wiki-pack"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn temp_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("wiki-pack-signing-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_pack(path: &Path, origin: Origin) {
    let payload = b"offline article bytes";
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: "signed-pack".into(),
        project: "enwiki".into(),
        snapshot: "20261101".into(),
        origin,
        metadata: PackMetadata::synthetic_fixture(),
        objects: vec![Object {
            path: "articles/1.shard".into(),
            sha256: sha256_hex(payload),
            bytes: payload.len() as u64,
        }],
    };
    let mut bytes = Vec::new();
    write_container(&mut bytes, &manifest, |_, out| out.write_all(payload)).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn generate_sign_and_verify_with_a_pinned_key_and_refuse_everything_else() {
    let dir = temp_dir();
    let key = dir.join("publisher.key");
    let generated = run(&[OsStr::new("signing-key-generate"), key.as_os_str()]);
    assert!(generated.status.success(), "{}", text(&generated.stderr));
    let printed = text(&generated.stdout);
    let public = printed
        .lines()
        .find_map(|line| line.strip_prefix("public-key "))
        .unwrap()
        .to_owned();
    // The secret is written once, privately, and never printed.
    let secret_hex = fs::read_to_string(&key)
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .to_owned();
    assert_eq!(secret_hex.len(), 64);
    assert!(!printed.contains(&secret_hex));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let again = run(&[OsStr::new("signing-key-generate"), key.as_os_str()]);
    assert!(
        !again.status.success(),
        "an existing key must never be overwritten"
    );

    let pack = dir.join("official.wpack");
    write_pack(
        &pack,
        Origin::Official {
            publisher: "foundation".into(),
        },
    );
    let signed = run(&[OsStr::new("sign-pack"), key.as_os_str(), pack.as_os_str()]);
    assert!(signed.status.success(), "{}", text(&signed.stderr));
    assert!(!text(&signed.stdout).contains(&secret_hex));
    let sig = dir.join("official.wpack.sig");
    assert!(sig.is_file());

    let verify = |pack: &Path, key: Option<(&str, &str)>| {
        let mut args = vec![
            OsStr::new("verify-pack-signature"),
            pack.as_os_str(),
            sig.as_os_str(),
        ];
        if let Some((publisher, public)) = key {
            args.extend([
                OsStr::new(publisher),
                OsStr::new(public),
                OsStr::new("20260101"),
            ]);
        }
        run(&args)
    };
    let ok = verify(&pack, Some(("foundation", &public)));
    assert!(ok.status.success(), "{}", text(&ok.stderr));
    assert!(text(&ok.stdout).contains("Verified publisher: foundation"));
    // The app's keyring pins no key yet; another publisher's key is refused.
    let unpinned = verify(&pack, None);
    assert!(text(&unpinned.stderr).contains("unrecognized publisher key"));
    let other = verify(&pack, Some(("someone-else", &public)));
    assert!(text(&other.stderr).contains("does not match the pack's publisher"));

    // A tampered pack neither verifies nor gets signed.
    let tampered = dir.join("tampered.wpack");
    let mut bytes = fs::read(&pack).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&tampered, bytes).unwrap();
    assert!(!verify(&tampered, Some(("foundation", &public)))
        .status
        .success());
    let refused = run(&[
        OsStr::new("sign-pack"),
        key.as_os_str(),
        tampered.as_os_str(),
    ]);
    assert!(text(&refused.stderr).contains("failed verification"));
    assert!(!dir.join("tampered.wpack.sig").exists());

    // Custom packs are never signed as official.
    let custom = dir.join("custom.wpack");
    write_pack(
        &custom,
        Origin::Custom {
            definition_id: "physics".into(),
        },
    );
    let custom_sign = run(&[OsStr::new("sign-pack"), key.as_os_str(), custom.as_os_str()]);
    assert!(text(&custom_sign.stderr).contains("does not claim official origin"));

    // A signing key readable by others is refused.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(&sig).unwrap();
        let loose = run(&[OsStr::new("sign-pack"), key.as_os_str(), pack.as_os_str()]);
        assert!(text(&loose.stderr).contains("owner only"));
    }
    fs::remove_dir_all(dir).ok();
}
