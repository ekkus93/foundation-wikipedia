//! Official publisher signatures for `.wpack` packs (TRUST-001).
//!
//! Owner decision (2026-11-16): Ed25519 (`ed25519-dalek`) in this crate's own
//! small format, with publisher public keys pinned by the app. The signature
//! covers the pack's canonical manifest identity transcript (ADR 0007), which
//! commits to every object's path, SHA-256 and length, so a valid signature
//! authenticates the whole pack once its bytes are verified against that
//! manifest. The signature travels in a separate file next to the pack (for
//! downloads, beside it in the catalog), leaving the accepted `FWPACK01`
//! framing unchanged.
//!
//! Signature file (UTF-8, LF line endings, exactly these four lines):
//!
//! ```text
//! foundation-wikipedia/pack-signature/v1
//! key-id <16 lowercase hex: first 8 bytes of SHA-256 of the public key>
//! transcript-sha256 <64 lowercase hex>
//! signature <128 lowercase hex: Ed25519 over SIGNED_PREFIX || transcript SHA-256>
//! ```
//!
//! A signature proves origin under the pinned-key policy only. It is not
//! permission to execute pack content, and every object byte must still be
//! verified (spec §8, §21).

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::manifest::{Manifest, ManifestError, Origin};

const HEADER: &str = "foundation-wikipedia/pack-signature/v1";
/// Domain separation: these bytes are never a valid signed message elsewhere.
pub const SIGNED_PREFIX: &[u8] = b"foundation-wikipedia/pack-signature/v1\0";
/// A signature file is four short lines; anything longer is refused unread.
pub const MAX_SIGNATURE_FILE_BYTES: usize = 512;

/// A publisher key the app trusts, compiled in (pinned).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedKey {
    /// Must equal the official manifest's `publisher`.
    pub publisher: &'static str,
    pub public_key: [u8; 32],
    /// Inclusive `YYYYMMDD` snapshot dates this key may sign; a rotated-out
    /// key keeps verifying the packs it signed but not newer snapshots.
    pub valid_from: &'static str,
    pub valid_until: Option<&'static str>,
    /// A compromised key: nothing it signed is trusted any more.
    pub revoked: bool,
}

impl TrustedKey {
    pub fn key_id(&self) -> [u8; 8] {
        key_id(&self.public_key)
    }
}

/// The production keyring. Empty until the publisher key is generated and
/// its public half pinned here (the private key never enters this
/// repository); until then no pack can show "Verified publisher".
pub const OFFICIAL_KEYS: &[TrustedKey] = &[];

/// A parsed signature file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackSignature {
    pub key_id: [u8; 8],
    pub transcript_sha256: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureError {
    /// The file is not exactly the v1 format (or too large).
    Malformed,
    /// The pack does not claim official origin, so there is nothing to verify.
    NotOfficial,
    /// The manifest itself is invalid.
    Manifest,
    /// The signature is for a different manifest.
    TranscriptMismatch,
    /// No pinned key has this ID: an unrecognized key never confers trust.
    UnknownKey,
    /// The key was revoked.
    RevokedKey,
    /// The key belongs to a different publisher than the manifest claims.
    PublisherMismatch,
    /// The snapshot is not a `YYYYMMDD` date inside the key's validity.
    OutsideValidity,
    /// The Ed25519 signature does not verify.
    BadSignature,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed pack signature",
            Self::NotOfficial => "pack does not claim official origin",
            Self::Manifest => "invalid pack manifest",
            Self::TranscriptMismatch => "signature is for a different pack",
            Self::UnknownKey => "unrecognized publisher key",
            Self::RevokedKey => "revoked publisher key",
            Self::PublisherMismatch => "publisher key does not match the pack's publisher",
            Self::OutsideValidity => "pack snapshot is outside the key's validity",
            Self::BadSignature => "invalid publisher signature",
        })
    }
}

impl From<ManifestError> for SignatureError {
    fn from(_: ManifestError) -> Self {
        Self::Manifest
    }
}

/// First 8 bytes of SHA-256 of the public key.
pub fn key_id(public_key: &[u8; 32]) -> [u8; 8] {
    let digest = Sha256::digest(public_key);
    let mut id = [0; 8];
    id.copy_from_slice(&digest[..8]);
    id
}

fn transcript_digest(manifest: &Manifest) -> Result<[u8; 32], SignatureError> {
    Ok(Sha256::digest(manifest.identity_transcript()?).into())
}

fn signed_message(transcript_sha256: &[u8; 32]) -> Vec<u8> {
    let mut message = SIGNED_PREFIX.to_vec();
    message.extend_from_slice(transcript_sha256);
    message
}

/// Lowercase hex, as used for key IDs in signature files and records.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Strict lowercase hex of exactly `N` bytes.
fn unhex<const N: usize>(text: &str) -> Result<[u8; N], SignatureError> {
    if text.len() != 2 * N {
        return Err(SignatureError::Malformed);
    }
    let mut out = [0; N];
    for (index, pair) in text.as_bytes().chunks(2).enumerate() {
        let digit = |c: u8| match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            _ => Err(SignatureError::Malformed),
        };
        out[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(out)
}

impl PackSignature {
    /// The canonical file bytes (re-encoding a parsed file reproduces it).
    pub fn encode(&self) -> String {
        format!(
            "{HEADER}\nkey-id {}\ntranscript-sha256 {}\nsignature {}\n",
            hex(&self.key_id),
            hex(&self.transcript_sha256),
            hex(&self.signature)
        )
    }

    /// Parse exactly the canonical form; any other spelling is refused.
    pub fn parse(bytes: &[u8]) -> Result<Self, SignatureError> {
        if bytes.len() > MAX_SIGNATURE_FILE_BYTES {
            return Err(SignatureError::Malformed);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| SignatureError::Malformed)?;
        let body = text.strip_suffix('\n').ok_or(SignatureError::Malformed)?;
        let lines: Vec<&str> = body.split('\n').collect();
        let [header, key, transcript, signature] = lines.as_slice() else {
            return Err(SignatureError::Malformed);
        };
        if *header != HEADER {
            return Err(SignatureError::Malformed);
        }
        let parsed = Self {
            key_id: unhex(field(key, "key-id")?)?,
            transcript_sha256: unhex(field(transcript, "transcript-sha256")?)?,
            signature: unhex(field(signature, "signature")?)?,
        };
        if parsed.encode().as_bytes() != bytes {
            return Err(SignatureError::Malformed);
        }
        Ok(parsed)
    }
}

/// The public key of a signing (secret) key, for pinning and tooling.
pub fn public_key(secret_key: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(secret_key)
        .verifying_key()
        .to_bytes()
}

/// Sign an official pack's manifest (release tooling and tests; the
/// production private key is held by the publisher, never in this repo).
pub fn sign(manifest: &Manifest, secret_key: &[u8; 32]) -> Result<PackSignature, SignatureError> {
    if !matches!(manifest.origin, Origin::Official { .. }) {
        return Err(SignatureError::NotOfficial);
    }
    let signing = SigningKey::from_bytes(secret_key);
    let transcript_sha256 = transcript_digest(manifest)?;
    Ok(PackSignature {
        key_id: key_id(&signing.verifying_key().to_bytes()),
        transcript_sha256,
        signature: signing.sign(&signed_message(&transcript_sha256)).to_bytes(),
    })
}

/// The value of a `name value` line.
fn field<'a>(line: &'a str, name: &str) -> Result<&'a str, SignatureError> {
    line.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(' '))
        .ok_or(SignatureError::Malformed)
}

fn is_date(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit())
}

/// Verify that a pinned, unrevoked key of the manifest's publisher signed
/// exactly this manifest for a snapshot inside the key's validity. Object
/// bytes must be verified separately against the same manifest.
pub fn verify(
    manifest: &Manifest,
    signature: &PackSignature,
    keyring: &[TrustedKey],
) -> Result<&'static str, SignatureError> {
    let Origin::Official { publisher } = &manifest.origin else {
        return Err(SignatureError::NotOfficial);
    };
    if transcript_digest(manifest)? != signature.transcript_sha256 {
        return Err(SignatureError::TranscriptMismatch);
    }
    let key = keyring
        .iter()
        .find(|key| key.key_id() == signature.key_id)
        .ok_or(SignatureError::UnknownKey)?;
    if key.revoked {
        return Err(SignatureError::RevokedKey);
    }
    if key.publisher != publisher {
        return Err(SignatureError::PublisherMismatch);
    }
    let snapshot = manifest.snapshot.as_str();
    let inside = is_date(snapshot)
        && is_date(key.valid_from)
        && snapshot >= key.valid_from
        && key
            .valid_until
            .is_none_or(|until| is_date(until) && snapshot <= until);
    if !inside {
        return Err(SignatureError::OutsideValidity);
    }
    let verifying =
        VerifyingKey::from_bytes(&key.public_key).map_err(|_| SignatureError::BadSignature)?;
    verifying
        .verify_strict(
            &signed_message(&signature.transcript_sha256),
            &Signature::from_bytes(&signature.signature),
        )
        .map_err(|_| SignatureError::BadSignature)?;
    Ok(key.publisher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Object, PackMetadata, FORMAT_VERSION};

    /// Throwaway test keys: never pinned in `OFFICIAL_KEYS`.
    const SECRET: [u8; 32] = [7; 32];
    const OTHER_SECRET: [u8; 32] = [9; 32];

    fn public(secret: &[u8; 32]) -> [u8; 32] {
        SigningKey::from_bytes(secret).verifying_key().to_bytes()
    }

    fn trusted(secret: &[u8; 32]) -> TrustedKey {
        TrustedKey {
            publisher: "foundation",
            public_key: public(secret),
            valid_from: "20260101",
            valid_until: Some("20271231"),
            revoked: false,
        }
    }

    fn official() -> Manifest {
        Manifest {
            version: FORMAT_VERSION,
            pack_id: "physics".into(),
            project: "enwiki".into(),
            snapshot: "20261009".into(),
            origin: Origin::Official {
                publisher: "foundation".into(),
            },
            metadata: PackMetadata::synthetic_fixture(),
            objects: vec![Object {
                path: "articles/42.cbor.zst".into(),
                sha256: "a".repeat(64),
                bytes: 1024,
            }],
        }
    }

    #[test]
    fn a_pinned_publisher_key_verifies_its_signature_and_the_file_round_trips() {
        let manifest = official();
        let signature = sign(&manifest, &SECRET).unwrap();
        assert_eq!(
            verify(&manifest, &signature, &[trusted(&SECRET)]),
            Ok("foundation")
        );
        let file = signature.encode();
        assert_eq!(PackSignature::parse(file.as_bytes()), Ok(signature));
        // Deterministic (Ed25519): a fixed key and manifest give fixed bytes.
        assert_eq!(sign(&manifest, &SECRET).unwrap().encode(), file);
    }

    #[test]
    fn tampering_with_the_manifest_or_signature_is_refused() {
        let manifest = official();
        let signature = sign(&manifest, &SECRET).unwrap();
        let keyring = [trusted(&SECRET)];
        let mut changed = manifest.clone();
        changed.objects[0].sha256 = "b".repeat(64);
        assert_eq!(
            verify(&changed, &signature, &keyring),
            Err(SignatureError::TranscriptMismatch)
        );
        let mut forged = signature.clone();
        forged.signature[0] ^= 1;
        assert_eq!(
            verify(&manifest, &forged, &keyring),
            Err(SignatureError::BadSignature)
        );
        // A digest that matches a changed manifest cannot reuse the signature.
        let mut swapped = signature.clone();
        swapped.transcript_sha256 = transcript_digest(&changed).unwrap();
        assert_eq!(
            verify(&changed, &swapped, &keyring),
            Err(SignatureError::BadSignature)
        );
    }

    #[test]
    fn only_pinned_unrevoked_keys_of_the_same_publisher_within_validity_are_trusted() {
        let manifest = official();
        let signature = sign(&manifest, &SECRET).unwrap();
        assert_eq!(
            verify(&manifest, &signature, &[]),
            Err(SignatureError::UnknownKey)
        );
        assert_eq!(
            verify(&manifest, &signature, &[trusted(&OTHER_SECRET)]),
            Err(SignatureError::UnknownKey)
        );
        let revoked = TrustedKey {
            revoked: true,
            ..trusted(&SECRET)
        };
        assert_eq!(
            verify(&manifest, &signature, &[revoked]),
            Err(SignatureError::RevokedKey)
        );
        let other_publisher = TrustedKey {
            publisher: "someone-else",
            ..trusted(&SECRET)
        };
        assert_eq!(
            verify(&manifest, &signature, &[other_publisher]),
            Err(SignatureError::PublisherMismatch)
        );
        // Rotation: a key retired before this snapshot no longer signs it.
        let retired = TrustedKey {
            valid_until: Some("20260930"),
            ..trusted(&SECRET)
        };
        assert_eq!(
            verify(&manifest, &signature, &[retired]),
            Err(SignatureError::OutsideValidity)
        );
        let mut undated = manifest.clone();
        undated.snapshot = "latest".into();
        let undated_signature = sign(&undated, &SECRET).unwrap();
        assert_eq!(
            verify(&undated, &undated_signature, &[trusted(&SECRET)]),
            Err(SignatureError::OutsideValidity)
        );
        // Custom packs are never signed or verified as official.
        let mut custom = manifest.clone();
        custom.origin = Origin::Custom {
            definition_id: "physics".into(),
        };
        assert_eq!(sign(&custom, &SECRET), Err(SignatureError::NotOfficial));
        assert_eq!(
            verify(&custom, &signature, &[trusted(&SECRET)]),
            Err(SignatureError::NotOfficial)
        );
        // The production keyring trusts nothing until a key is pinned.
        assert_eq!(
            verify(&manifest, &signature, OFFICIAL_KEYS),
            Err(SignatureError::UnknownKey)
        );
    }

    /// Committed test vectors (throwaway key `[7; 32]`), so other
    /// implementations can check transcript, message and signature bytes.
    /// Regenerate after an intentional change with `WPACK_WRITE_FIXTURE=1
    /// cargo test -p wiki-pack-format committed_signature_test_vectors` and
    /// update the provenance hashes.
    #[test]
    fn committed_signature_test_vectors() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/signature");
        let manifest = official();
        let files = [
            (
                "pack-signature-v1.transcript.bin",
                manifest.identity_transcript().unwrap(),
            ),
            (
                "pack-signature-v1.sig",
                sign(&manifest, &SECRET).unwrap().encode().into_bytes(),
            ),
            (
                "pack-signature-v1.public-key.hex",
                format!("{}\n", hex(&public(&SECRET))).into_bytes(),
            ),
        ];
        if std::env::var_os("WPACK_WRITE_FIXTURE").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            for (name, bytes) in &files {
                std::fs::write(dir.join(name), bytes).unwrap();
            }
        }
        for (name, bytes) in &files {
            assert_eq!(&std::fs::read(dir.join(name)).unwrap(), bytes, "{name}");
        }
        let committed = PackSignature::parse(&files[1].1).unwrap();
        assert_eq!(
            verify(&manifest, &committed, &[trusted(&SECRET)]),
            Ok("foundation")
        );
    }

    #[test]
    fn only_the_exact_canonical_file_parses() {
        let file = sign(&official(), &SECRET).unwrap().encode();
        let bad = [
            file.replace('\n', "\r\n"),
            file.to_uppercase(),
            file.trim_end().to_owned(),
            format!("{file}\n"),
            file.replace("key-id ", "key-id  "),
            file.replacen("pack-signature/v1", "pack-signature/v2", 1),
            format!("{file}extra\n"),
            "x".repeat(MAX_SIGNATURE_FILE_BYTES + 1),
            String::new(),
        ];
        for text in bad {
            assert_eq!(
                PackSignature::parse(text.as_bytes()),
                Err(SignatureError::Malformed),
                "{text:?}"
            );
        }
    }
}
