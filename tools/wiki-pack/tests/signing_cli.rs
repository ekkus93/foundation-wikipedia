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
    // A throwaway key is not in the app's keyring; another publisher's key is refused.
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

#[test]
fn export_official_writes_an_unsigned_official_pack_that_sign_pack_then_signs() {
    let dir = temp_dir();
    let objects = dir.join("objects");
    let out = dir.join("out");
    for path in [&dir, &objects, &out] {
        fs::create_dir_all(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    fs::write(objects.join("article.cbor"), b"article").unwrap();
    let meta = dir.join("meta.json");
    let zero = "0".repeat(64);
    fs::write(
        &meta,
        format!(
            r#"{{"source":{{"kind":"custom","snapshot_id":"fixture","manifest_sha256":"{zero}"}},"definition_sha256":"{zero}","license":{{"text_spdx":"CC-BY-SA-4.0","attribution_object":null}},"indexes":[],"counts":{{"articles":0,"redirects":0,"media":0}}}}"#
        ),
    )
    .unwrap();
    let pack = out.join("official.wpack");
    let exported = run(&[
        OsStr::new("export-official"),
        objects.as_os_str(),
        OsStr::new("science"),
        OsStr::new("enwiki"),
        OsStr::new("20261116"),
        OsStr::new("foundation"),
        meta.as_os_str(),
        pack.as_os_str(),
    ]);
    assert!(exported.status.success(), "{}", text(&exported.stderr));
    assert!(text(&exported.stdout).contains("Official envelope written (UNSIGNED)"));

    let key = dir.join("throwaway.key");
    let generated = run(&[OsStr::new("signing-key-generate"), key.as_os_str()]);
    let public = text(&generated.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("public-key ").map(str::to_owned))
        .unwrap();
    let signed = run(&[OsStr::new("sign-pack"), key.as_os_str(), pack.as_os_str()]);
    assert!(signed.status.success(), "{}", text(&signed.stderr));
    let sig = out.join("official.wpack.sig");
    let verified = run(&[
        OsStr::new("verify-pack-signature"),
        pack.as_os_str(),
        sig.as_os_str(),
        OsStr::new("foundation"),
        OsStr::new(&public),
        OsStr::new("20260101"),
    ]);
    assert!(verified.status.success(), "{}", text(&verified.stderr));
    assert!(text(&verified.stdout).contains("Verified publisher: foundation"));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_catalog_built_from_a_signed_pack_signs_verifies_and_refuses_tampering() {
    let dir = temp_dir();
    let key = dir.join("publisher.key");
    let generated = run(&[OsStr::new("signing-key-generate"), key.as_os_str()]);
    let public = text(&generated.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("public-key "))
        .unwrap()
        .to_owned();
    let pack = dir.join("official.wpack");
    write_pack(
        &pack,
        Origin::Official {
            publisher: "foundation".into(),
        },
    );
    assert!(
        run(&[OsStr::new("sign-pack"), key.as_os_str(), pack.as_os_str()])
            .status
            .success()
    );
    let pack_sig = dir.join("official.wpack.sig");

    let entry_out = run(&[
        OsStr::new("catalog-entry"),
        pack.as_os_str(),
        pack_sig.as_os_str(),
        OsStr::new("Signed sample"),
        OsStr::new("https://example.org/signed-pack.wpack"),
        OsStr::new("https://example.org/signed-pack.wpack.sig"),
    ]);
    assert!(entry_out.status.success(), "{}", text(&entry_out.stderr));
    let entry: serde_json::Value = serde_json::from_slice(&entry_out.stdout).unwrap();
    assert_eq!(entry["pack_id"], "signed-pack");
    assert_eq!(entry["bytes"], fs::metadata(&pack).unwrap().len());
    let signature_text = fs::read_to_string(&pack_sig).unwrap();
    let transcript = entry["transcript_sha256"].as_str().unwrap();
    assert!(signature_text.contains(&format!("transcript-sha256 {transcript}\n")));
    let entry_file = dir.join("entry.json");
    fs::write(&entry_file, &entry_out.stdout).unwrap();

    // An http URL is refused when building the entry.
    let http = run(&[
        OsStr::new("catalog-entry"),
        pack.as_os_str(),
        pack_sig.as_os_str(),
        OsStr::new("Signed sample"),
        OsStr::new("http://example.org/signed-pack.wpack"),
        OsStr::new("https://example.org/signed-pack.wpack.sig"),
    ]);
    assert!(!http.status.success());

    let catalog = dir.join("catalog.json");
    let built = run(&[
        OsStr::new("catalog-build"),
        OsStr::new("foundation"),
        OsStr::new("3"),
        OsStr::new("20261116"),
        catalog.as_os_str(),
        entry_file.as_os_str(),
    ]);
    assert!(built.status.success(), "{}", text(&built.stderr));
    let signed = run(&[
        OsStr::new("sign-catalog"),
        key.as_os_str(),
        catalog.as_os_str(),
    ]);
    assert!(signed.status.success(), "{}", text(&signed.stderr));
    let catalog_sig = dir.join("catalog.json.sig");

    let verify = |catalog: &Path, sig: &Path, pinned: bool| {
        let mut args = vec![
            OsStr::new("verify-catalog"),
            catalog.as_os_str(),
            sig.as_os_str(),
        ];
        if !pinned {
            args.extend([
                OsStr::new("foundation"),
                OsStr::new(public.as_str()),
                OsStr::new("20260101"),
            ]);
        }
        run(&args)
    };
    let ok = verify(&catalog, &catalog_sig, false);
    assert!(ok.status.success(), "{}", text(&ok.stderr));
    assert!(text(&ok.stdout).contains("Verified catalog: foundation sequence 3 (1 packs"));
    // The throwaway key is not pinned in the app.
    assert!(text(&verify(&catalog, &catalog_sig, true).stderr).contains("unrecognized"));
    // A pack signature is not a catalog signature.
    assert!(!verify(&catalog, &pack_sig, false).status.success());
    // One changed byte fails.
    let tampered = dir.join("tampered.json");
    fs::write(
        &tampered,
        fs::read_to_string(&catalog)
            .unwrap()
            .replace("Signed sample", "Signed sampla"),
    )
    .unwrap();
    assert!(text(&verify(&tampered, &catalog_sig, false).stderr).contains("different bytes"));
    // Neither the catalog nor its signature is overwritten.
    assert!(!run(&[
        OsStr::new("sign-catalog"),
        key.as_os_str(),
        catalog.as_os_str()
    ])
    .status
    .success());
    fs::remove_dir_all(dir).ok();
}
