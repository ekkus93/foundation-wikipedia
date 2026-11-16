//! Signed publisher catalog of official packs (MOBPACK-001, ADR 0009).
//!
//! The catalog is a small JSON document listing downloadable official
//! packs. Its signature covers the catalog's **exact bytes**: the signed
//! message is `CATALOG_SIGNED_PREFIX || SHA-256(catalog bytes)`, signed with
//! a pinned publisher key (ADR 0008). The prefix differs from the pack
//! signature prefix, so a pack signature can never pass as a catalog
//! signature or the reverse. The JSON is parsed only after the signature
//! verified, and then strictly (unknown fields, duplicates and oversize
//! values are refused).
//!
//! A catalog lists packs; it never makes one trusted. Each pack must still
//! verify against its own signature and every object byte before it is
//! installed. A monotonically increasing `sequence` lets clients refuse a
//! replayed older catalog.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::manifest::{safe_id, valid_wiki_project};
use crate::signature::{field, hex, is_date, key_id, unhex, SignatureError, TrustedKey};

const HEADER: &str = "foundation-wikipedia/catalog-signature/v1";
/// Domain separation from pack signatures (`pack-signature/v1`).
pub const CATALOG_SIGNED_PREFIX: &[u8] = b"foundation-wikipedia/catalog-signature/v1\0";
/// The catalog document's `format` value.
pub const CATALOG_FORMAT: &str = "foundation-wikipedia/catalog/v1";
/// A catalog larger than this is refused before hashing or parsing.
pub const MAX_CATALOG_BYTES: usize = 1 << 20;
pub const MAX_CATALOG_PACKS: usize = 1024;
const MAX_URL_BYTES: usize = 2048;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SIGNATURE_FILE_BYTES: usize = 512;

/// One downloadable official pack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPack {
    pub pack_id: String,
    pub project: String,
    /// `YYYYMMDD` Wikimedia snapshot.
    pub snapshot: String,
    pub title: String,
    /// HTTPS URL of the `.wpack`.
    pub url: String,
    /// Exact size of the `.wpack`, for disk estimates and download bounds.
    pub bytes: u64,
    /// HTTPS URL of the pack's `.wpack.sig`.
    pub signature_url: String,
    /// The pack's manifest identity-transcript SHA-256 (lowercase hex); its
    /// signature file must name the same transcript.
    pub transcript_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub format: String,
    pub publisher: String,
    /// Strictly increases with each publication; clients refuse older ones.
    pub sequence: u64,
    /// `YYYYMMDD` publication date; must lie inside the key's validity.
    pub generated: String,
    pub packs: Vec<CatalogPack>,
}

/// The catalog's detached signature (`catalog.json.sig`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSignature {
    pub key_id: [u8; 8],
    pub catalog_sha256: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// Larger than `MAX_CATALOG_BYTES`.
    TooLarge,
    /// The signature file is malformed, or the key/signature is not trusted.
    Signature(SignatureError),
    /// The signature is for different catalog bytes.
    DigestMismatch,
    /// Signed, but not a valid v1 catalog (the reason is for diagnostics).
    Invalid(&'static str),
    /// Older than a catalog this client already accepted.
    Rollback { accepted: u64, offered: u64 },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("catalog is too large"),
            Self::Signature(error) => write!(f, "catalog signature: {error}"),
            Self::DigestMismatch => f.write_str("catalog signature is for different bytes"),
            Self::Invalid(reason) => write!(f, "invalid catalog: {reason}"),
            Self::Rollback { accepted, offered } => write!(
                f,
                "catalog sequence {offered} is older than accepted sequence {accepted}"
            ),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<SignatureError> for CatalogError {
    fn from(error: SignatureError) -> Self {
        Self::Signature(error)
    }
}

fn signed_message(digest: &[u8; 32]) -> Vec<u8> {
    let mut message = CATALOG_SIGNED_PREFIX.to_vec();
    message.extend_from_slice(digest);
    message
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

impl CatalogSignature {
    pub fn encode(&self) -> String {
        format!(
            "{HEADER}\nkey-id {}\ncatalog-sha256 {}\nsignature {}\n",
            hex(&self.key_id),
            hex(&self.catalog_sha256),
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
        let [header, key, catalog, signature] = lines.as_slice() else {
            return Err(SignatureError::Malformed);
        };
        if *header != HEADER {
            return Err(SignatureError::Malformed);
        }
        let parsed = Self {
            key_id: unhex(field(key, "key-id")?)?,
            catalog_sha256: unhex(field(catalog, "catalog-sha256")?)?,
            signature: unhex(field(signature, "signature")?)?,
        };
        if parsed.encode().as_bytes() != bytes {
            return Err(SignatureError::Malformed);
        }
        Ok(parsed)
    }
}

fn https_url(value: &str) -> bool {
    value.len() <= MAX_URL_BYTES
        && value.starts_with("https://")
        && value.len() > "https://".len()
        && value
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\\' | b'<' | b'>' | b'`'))
}

fn lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Catalog {
    /// Structural checks; signature and publisher checks happen in `verify`.
    pub fn validate(&self) -> Result<(), CatalogError> {
        use CatalogError::Invalid;
        if self.format != CATALOG_FORMAT {
            return Err(Invalid("unsupported format"));
        }
        if !safe_id(&self.publisher) || self.publisher.is_empty() {
            return Err(Invalid("publisher"));
        }
        if self.sequence == 0 {
            return Err(Invalid("sequence must be positive"));
        }
        if !is_date(&self.generated) {
            return Err(Invalid("generated must be YYYYMMDD"));
        }
        if self.packs.len() > MAX_CATALOG_PACKS {
            return Err(Invalid("too many packs"));
        }
        let mut seen = std::collections::HashSet::new();
        for pack in &self.packs {
            if pack.pack_id.is_empty() || !safe_id(&pack.pack_id) {
                return Err(Invalid("pack_id"));
            }
            if !seen.insert(pack.pack_id.as_str()) {
                return Err(Invalid("duplicate pack_id"));
            }
            if !valid_wiki_project(&pack.project) {
                return Err(Invalid("project"));
            }
            if !is_date(&pack.snapshot) {
                return Err(Invalid("snapshot must be YYYYMMDD"));
            }
            let title_chars = pack.title.chars().count();
            if title_chars == 0
                || title_chars > MAX_TITLE_CHARS
                || pack.title.trim() != pack.title
                || pack.title.chars().any(char::is_control)
            {
                return Err(Invalid("title"));
            }
            if !https_url(&pack.url) || !https_url(&pack.signature_url) {
                return Err(Invalid("pack URLs must be HTTPS"));
            }
            if pack.bytes == 0 {
                return Err(Invalid("pack size"));
            }
            if !lower_hex_64(&pack.transcript_sha256) {
                return Err(Invalid("transcript_sha256"));
            }
        }
        Ok(())
    }

    /// The bytes to publish: pretty JSON with a trailing newline. Any bytes
    /// verify as long as they were signed; this is just the tool's output.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CatalogError> {
        self.validate()?;
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|_| CatalogError::Invalid("encoding"))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// Sign catalog bytes (release tooling and tests). The bytes must already be
/// a valid catalog so a publisher never signs something clients refuse.
pub fn sign(catalog_bytes: &[u8], secret_key: &[u8; 32]) -> Result<CatalogSignature, CatalogError> {
    parse_unverified(catalog_bytes)?;
    let signing = SigningKey::from_bytes(secret_key);
    let catalog_sha256 = digest(catalog_bytes);
    Ok(CatalogSignature {
        key_id: key_id(&signing.verifying_key().to_bytes()),
        catalog_sha256,
        signature: signing.sign(&signed_message(&catalog_sha256)).to_bytes(),
    })
}

fn parse_unverified(catalog_bytes: &[u8]) -> Result<Catalog, CatalogError> {
    if catalog_bytes.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::TooLarge);
    }
    let catalog: Catalog = serde_json::from_slice(catalog_bytes)
        .map_err(|_| CatalogError::Invalid("not a v1 catalog document"))?;
    catalog.validate()?;
    Ok(catalog)
}

/// Verify catalog bytes against their signature and a pinned keyring, then
/// parse them. `accepted_sequence` is the highest sequence this client has
/// already accepted (if any); an older catalog is refused, the same one is
/// allowed (a re-fetch).
pub fn verify(
    catalog_bytes: &[u8],
    signature: &CatalogSignature,
    keyring: &[TrustedKey],
    accepted_sequence: Option<u64>,
) -> Result<Catalog, CatalogError> {
    if catalog_bytes.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::TooLarge);
    }
    if digest(catalog_bytes) != signature.catalog_sha256 {
        return Err(CatalogError::DigestMismatch);
    }
    let key = keyring
        .iter()
        .find(|key| key.key_id() == signature.key_id)
        .ok_or(SignatureError::UnknownKey)?;
    if key.revoked {
        return Err(SignatureError::RevokedKey.into());
    }
    let verifying =
        VerifyingKey::from_bytes(&key.public_key).map_err(|_| SignatureError::BadSignature)?;
    verifying
        .verify_strict(
            &signed_message(&signature.catalog_sha256),
            &Signature::from_bytes(&signature.signature),
        )
        .map_err(|_| SignatureError::BadSignature)?;
    // Only now is the document parsed.
    let catalog = parse_unverified(catalog_bytes)?;
    if catalog.publisher != key.publisher {
        return Err(SignatureError::PublisherMismatch.into());
    }
    let generated = catalog.generated.as_str();
    let inside = is_date(key.valid_from)
        && generated >= key.valid_from
        && key
            .valid_until
            .is_none_or(|until| is_date(until) && generated <= until);
    if !inside {
        return Err(SignatureError::OutsideValidity.into());
    }
    if let Some(accepted) = accepted_sequence {
        if catalog.sequence < accepted {
            return Err(CatalogError::Rollback {
                accepted,
                offered: catalog.sequence,
            });
        }
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::PackSignature;

    /// Throwaway test keys: never pinned in `OFFICIAL_KEYS`.
    const SECRET: [u8; 32] = [7; 32];
    const OTHER_SECRET: [u8; 32] = [9; 32];

    fn trusted(secret: &[u8; 32]) -> TrustedKey {
        TrustedKey {
            publisher: "foundation",
            public_key: crate::signature::public_key(secret),
            valid_from: "20260101",
            valid_until: Some("20271231"),
            revoked: false,
        }
    }

    fn pack(id: &str) -> CatalogPack {
        CatalogPack {
            pack_id: id.into(),
            project: "enwiki".into(),
            snapshot: "20261101".into(),
            title: "Physics".into(),
            url: format!("https://example.org/{id}.wpack"),
            bytes: 4096,
            signature_url: format!("https://example.org/{id}.wpack.sig"),
            transcript_sha256: "ab".repeat(32),
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            format: CATALOG_FORMAT.into(),
            publisher: "foundation".into(),
            sequence: 5,
            generated: "20261116".into(),
            packs: vec![pack("physics"), pack("biology")],
        }
    }

    fn signed(catalog: &Catalog) -> (Vec<u8>, CatalogSignature) {
        let bytes = catalog.to_bytes().unwrap();
        let signature = sign(&bytes, &SECRET).unwrap();
        (bytes, signature)
    }

    #[test]
    fn a_signed_catalog_verifies_and_round_trips() {
        let (bytes, signature) = signed(&catalog());
        let encoded = signature.encode();
        assert_eq!(
            CatalogSignature::parse(encoded.as_bytes()).unwrap(),
            signature
        );
        let verified = verify(&bytes, &signature, &[trusted(&SECRET)], None).unwrap();
        assert_eq!(verified, catalog());
        // Re-fetching the same sequence is fine.
        verify(&bytes, &signature, &[trusted(&SECRET)], Some(5)).unwrap();
    }

    #[test]
    fn tampering_unknown_revoked_and_wrong_publisher_keys_are_refused() {
        let (bytes, signature) = signed(&catalog());
        let keyring = [trusted(&SECRET)];

        let mut tampered = bytes.clone();
        let at = tampered.iter().position(|b| *b == b'4').unwrap(); // a size digit
        tampered[at] = b'9';
        assert_eq!(
            verify(&tampered, &signature, &keyring, None),
            Err(CatalogError::DigestMismatch)
        );

        let mut forged = signature.clone();
        forged.signature[0] ^= 1;
        assert_eq!(
            verify(&bytes, &forged, &keyring, None),
            Err(CatalogError::Signature(SignatureError::BadSignature))
        );

        assert_eq!(
            verify(&bytes, &signature, &[trusted(&OTHER_SECRET)], None),
            Err(CatalogError::Signature(SignatureError::UnknownKey))
        );
        let revoked = TrustedKey {
            revoked: true,
            ..trusted(&SECRET)
        };
        assert_eq!(
            verify(&bytes, &signature, &[revoked], None),
            Err(CatalogError::Signature(SignatureError::RevokedKey))
        );
        let other_publisher = TrustedKey {
            publisher: "someone-else",
            ..trusted(&SECRET)
        };
        assert_eq!(
            verify(&bytes, &signature, &[other_publisher], None),
            Err(CatalogError::Signature(SignatureError::PublisherMismatch))
        );
        let expired = TrustedKey {
            valid_until: Some("20261031"),
            ..trusted(&SECRET)
        };
        assert_eq!(
            verify(&bytes, &signature, &[expired], None),
            Err(CatalogError::Signature(SignatureError::OutsideValidity))
        );
    }

    #[test]
    fn an_older_catalog_is_refused_as_rollback() {
        let (bytes, signature) = signed(&catalog());
        assert_eq!(
            verify(&bytes, &signature, &[trusted(&SECRET)], Some(6)),
            Err(CatalogError::Rollback {
                accepted: 6,
                offered: 5
            })
        );
    }

    #[test]
    fn a_pack_signature_never_passes_as_a_catalog_signature() {
        // Same key, same 32-byte digest, but signed under the pack prefix.
        let (bytes, _) = signed(&catalog());
        let digest = digest(&bytes);
        let mut pack_message = crate::signature::SIGNED_PREFIX.to_vec();
        pack_message.extend_from_slice(&digest);
        let cross = CatalogSignature {
            key_id: key_id(&crate::signature::public_key(&SECRET)),
            catalog_sha256: digest,
            signature: SigningKey::from_bytes(&SECRET)
                .sign(&pack_message)
                .to_bytes(),
        };
        assert_eq!(
            verify(&bytes, &cross, &[trusted(&SECRET)], None),
            Err(CatalogError::Signature(SignatureError::BadSignature))
        );
        // And the two signature files can't be confused by their parsers.
        let pack_file = PackSignature {
            key_id: cross.key_id,
            transcript_sha256: digest,
            signature: cross.signature,
        }
        .encode();
        assert!(CatalogSignature::parse(pack_file.as_bytes()).is_err());
        assert!(PackSignature::parse(cross.encode().as_bytes()).is_err());
    }

    #[test]
    fn invalid_documents_are_refused_even_when_signed() {
        let keyring = [trusted(&SECRET)];
        let sign_raw = |bytes: &[u8]| {
            let signing = SigningKey::from_bytes(&SECRET);
            let catalog_sha256 = digest(bytes);
            CatalogSignature {
                key_id: key_id(&signing.verifying_key().to_bytes()),
                catalog_sha256,
                signature: signing.sign(&signed_message(&catalog_sha256)).to_bytes(),
            }
        };
        let mut cases: Vec<(Catalog, &str)> = Vec::new();
        let mut c = catalog();
        c.packs[0].url = "http://example.org/x.wpack".into();
        cases.push((c, "plain http"));
        let mut c = catalog();
        c.packs[1].pack_id = "physics".into();
        cases.push((c, "duplicate"));
        let mut c = catalog();
        c.packs[0].pack_id = "../escape".into();
        cases.push((c, "path"));
        let mut c = catalog();
        c.sequence = 0;
        cases.push((c, "sequence"));
        let mut c = catalog();
        c.packs[0].transcript_sha256 = "AB".repeat(32);
        cases.push((c, "uppercase hex"));
        let mut c = catalog();
        c.packs[0].bytes = 0;
        cases.push((c, "size"));
        let mut c = catalog();
        c.format = "foundation-wikipedia/catalog/v2".into();
        cases.push((c, "format"));
        for (case, name) in cases {
            assert!(case.to_bytes().is_err(), "{name}: the tool refuses it");
            let bytes = serde_json::to_vec(&case).unwrap();
            assert!(
                matches!(
                    verify(&bytes, &sign_raw(&bytes), &keyring, None),
                    Err(CatalogError::Invalid(_))
                ),
                "{name}"
            );
        }
        let unknown_field = br#"{"format":"foundation-wikipedia/catalog/v1","publisher":"foundation","sequence":1,"generated":"20261116","packs":[],"mirror":"x"}"#;
        assert!(matches!(
            verify(unknown_field, &sign_raw(unknown_field), &keyring, None),
            Err(CatalogError::Invalid(_))
        ));
        let too_large = vec![b' '; MAX_CATALOG_BYTES + 1];
        assert_eq!(
            verify(&too_large, &sign_raw(&too_large), &keyring, None),
            Err(CatalogError::TooLarge)
        );
    }

    #[test]
    fn malformed_signature_files_are_refused() {
        let (_, signature) = signed(&catalog());
        let good = signature.encode();
        for bad in [
            good.replace("key-id ", "key-id  "),
            good.to_uppercase(),
            good.trim_end().to_owned(),
            format!("{good}\n"),
            good.replace(HEADER, "foundation-wikipedia/pack-signature/v1"),
        ] {
            assert_eq!(
                CatalogSignature::parse(bad.as_bytes()),
                Err(SignatureError::Malformed),
                "{bad:?}"
            );
        }
    }
}
