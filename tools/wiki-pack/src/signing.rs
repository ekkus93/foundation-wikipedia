//! TRUST-001 publisher signing commands (ADR 0008). Run them on the
//! publisher's offline machine: the secret key never leaves its file, and
//! nothing here prints it.

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use wiki_pack_builder::wiki_pack_format::container::verify_container;
use wiki_pack_builder::wiki_pack_format::signature::{
    self, hex, key_id, PackSignature, TrustedKey, MAX_SIGNATURE_FILE_BYTES, OFFICIAL_KEYS,
};

const KEY_HEADER: &str = "foundation-wikipedia/pack-signing-key/v1";

fn create_private(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|error| {
        format!(
            "cannot create {} (it must not exist yet): {error}",
            path.display()
        )
    })
}

fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0; 32];
    for (index, pair) in text.as_bytes().chunks(2).enumerate() {
        out[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

fn read_secret(path: &Path) -> Result<[u8; 32], String> {
    let meta = fs::symlink_metadata(path).map_err(|error| format!("cannot read key: {error}"))?;
    if !meta.is_file() || meta.len() > 256 {
        return Err("the signing key must be a small regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(
                "the signing key file must be readable by its owner only (chmod 600)".into(),
            );
        }
    }
    let text = fs::read_to_string(path).map_err(|error| format!("cannot read key: {error}"))?;
    text.strip_prefix(KEY_HEADER)
        .and_then(|rest| rest.strip_prefix('\n'))
        .and_then(|rest| rest.strip_suffix('\n'))
        .and_then(parse_hex32)
        .ok_or_else(|| "not a pack signing key file".into())
}

/// Create a new signing key from the OS random generator; print only its
/// public half and key ID.
pub fn generate_key(path: PathBuf) -> Result<(), String> {
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).map_err(|error| format!("no OS randomness: {error}"))?;
    let mut file = create_private(&path)?;
    file.write_all(format!("{KEY_HEADER}\n{}\n", hex(&secret)).as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write key: {error}"))?;
    let public = signature::public_key(&secret);
    println!("public-key {}", hex(&public));
    println!("key-id {}", hex(&key_id(&public)));
    println!("Keep {} offline; pin only the public key.", path.display());
    Ok(())
}

fn signature_path(pack: &Path) -> PathBuf {
    let mut name = pack.as_os_str().to_owned();
    name.push(".sig");
    PathBuf::from(name)
}

/// Verify every byte of an official pack, then write `<pack>.sig`.
pub fn sign_pack(key: PathBuf, pack: PathBuf) -> Result<(), String> {
    let secret = read_secret(&key)?;
    let file = File::open(&pack).map_err(|error| format!("cannot open pack: {error}"))?;
    let manifest = verify_container(&mut BufReader::new(file))
        .map_err(|error| format!("pack failed verification; not signed: {error:?}"))?;
    let signed =
        signature::sign(&manifest, &secret).map_err(|error| format!("not signed: {error}"))?;
    let out = signature_path(&pack);
    let mut file = create_private(&out)?;
    file.write_all(signed.encode().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write signature: {error}"))?;
    println!("key-id {}", hex(&signed.key_id));
    println!("signature {}", out.display());
    Ok(())
}

/// Check a pack and its signature, against one given key or the app's
/// pinned keyring.
pub fn verify_pack_signature(
    pack: PathBuf,
    signature_file: PathBuf,
    key: Option<(String, String, String)>,
) -> Result<(), String> {
    let meta =
        fs::metadata(&signature_file).map_err(|error| format!("cannot read signature: {error}"))?;
    if meta.len() > MAX_SIGNATURE_FILE_BYTES as u64 {
        return Err("malformed pack signature".into());
    }
    let bytes =
        fs::read(&signature_file).map_err(|error| format!("cannot read signature: {error}"))?;
    let parsed = PackSignature::parse(&bytes).map_err(|error| error.to_string())?;
    let file = File::open(&pack).map_err(|error| format!("cannot open pack: {error}"))?;
    let manifest = verify_container(&mut BufReader::new(file))
        .map_err(|error| format!("pack failed verification: {error:?}"))?;
    let given;
    let keyring: &[TrustedKey] = match key {
        Some((publisher, public_hex, valid_from)) => {
            let public_key =
                parse_hex32(&public_hex).ok_or("the public key must be 64 lowercase hex digits")?;
            given = [TrustedKey {
                publisher: Box::leak(publisher.into_boxed_str()),
                public_key,
                valid_from: Box::leak(valid_from.into_boxed_str()),
                valid_until: None,
                revoked: false,
            }];
            &given
        }
        None => OFFICIAL_KEYS,
    };
    let publisher =
        signature::verify(&manifest, &parsed, keyring).map_err(|error| error.to_string())?;
    println!(
        "Verified publisher: {publisher} (key {})",
        hex(&parsed.key_id)
    );
    Ok(())
}
