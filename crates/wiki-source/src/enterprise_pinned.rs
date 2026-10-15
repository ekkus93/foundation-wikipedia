//! Digest-pinned streaming Structured Contents import boundary.
//!
//! Caller-supplied digests are integrity pins, not proof that a Wikimedia
//! publisher authenticated either input or that a snapshot is complete.

use std::io::{self, BufRead, BufReader, Read};

use sha2::{Digest, Sha256};

use crate::enterprise::JoinedEnterpriseArticle;
use crate::enterprise_ndjson::InputKind;
use crate::enterprise_verified::{
    enterprise_normalize, enterprise_normalize_article, import_verified_enterprise_ndjson_scoped,
    VerifiedImportError,
};
use crate::join::DeletionMarker;

struct HashingInput<R> {
    source: R,
    hash: Sha256,
}

impl<R: Read> HashingInput<R> {
    fn new(source: R) -> Self {
        Self {
            source,
            hash: Sha256::new(),
        }
    }
}

impl<R: Read> Read for HashingInput<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.source.read(bytes)?;
        self.hash.update(&bytes[..count]);
        Ok(count)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinnedImportError {
    InvalidDigestPin,
    Import(VerifiedImportError),
    DigestMismatch(InputKind),
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == 64 && pin.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Authenticate the supplied pins out-of-band before trusting the result.
/// Both source streams are consumed to EOF and compared against the digests
/// from the exact bytes used by the revision-matched validation pipeline.
/// No successful batch is returned if either digest differs.
pub fn import_pinned_enterprise_ndjson_scoped<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    project: &str,
    generation_id: &str,
    deleted: Vec<DeletionMarker>,
    structured_sha256: &str,
    regular_sha256: &str,
) -> Result<Vec<JoinedEnterpriseArticle>, PinnedImportError> {
    if !valid_pin(structured_sha256) || !valid_pin(regular_sha256) {
        return Err(PinnedImportError::InvalidDigestPin);
    }
    let mut structured = HashingInput::new(structured);
    let mut regular = HashingInput::new(regular);
    let joined = import_verified_enterprise_ndjson_scoped(
        BufReader::new(&mut structured),
        BufReader::new(&mut regular),
        project,
        generation_id,
        deleted,
    )
    .map_err(PinnedImportError::Import)?;
    if format!("{:x}", structured.hash.finalize()) != structured_sha256.to_ascii_lowercase() {
        return Err(PinnedImportError::DigestMismatch(InputKind::Structured));
    }
    if format!("{:x}", regular.hash.finalize()) != regular_sha256.to_ascii_lowercase() {
        return Err(PinnedImportError::DigestMismatch(InputKind::Regular));
    }
    Ok(joined)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinnedCanonicalImportError {
    Import(PinnedImportError),
    Normalize(enterprise_normalize::NormalizeError),
}

/// Normalize only after both input members and all joined evidence agree.
pub fn import_pinned_canonical_enterprise_ndjson_scoped<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    project: &str,
    generation_id: &str,
    deleted: Vec<DeletionMarker>,
    structured_sha256: &str,
    regular_sha256: &str,
) -> Result<Vec<enterprise_normalize::CanonicalEnterpriseArticle>, PinnedCanonicalImportError> {
    let joined = import_pinned_enterprise_ndjson_scoped(
        structured,
        regular,
        project,
        generation_id,
        deleted,
        structured_sha256,
        regular_sha256,
    )
    .map_err(PinnedCanonicalImportError::Import)?;
    enterprise_normalize_article::normalize_enterprise_batch(&joined)
        .map_err(PinnedCanonicalImportError::Normalize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const STRUCTURED: &str = r#"{"name":"Example","identifier":1,"version":{"identifier":17},"is_part_of":{"identifier":"enwiki"},"in_language":{"identifier":"en"},"namespace":{"identifier":0},"date_modified":"2026-10-10T00:00:00Z"}"#;
    const REGULAR: &str = r#"{"name":"Example","identifier":1,"version":{"identifier":17},"is_part_of":{"identifier":"enwiki"},"in_language":{"identifier":"en"},"namespace":{"identifier":0},"date_modified":"2026-10-10T00:00:00Z","article_body":{"html":"<article>Example</article>"},"categories":[{"name":"Science"}],"redirects":[]}"#;

    fn pin(text: &str) -> String {
        format!("{:x}", Sha256::digest(text.as_bytes()))
    }

    #[test]
    fn accepts_digest_pinned_matching_components() {
        let result = import_pinned_enterprise_ndjson_scoped(
            Cursor::new(STRUCTURED),
            Cursor::new(REGULAR),
            "enwiki",
            "20261010",
            vec![],
            &pin(STRUCTURED),
            &pin(REGULAR),
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].structured.page.page_id, 1);
        assert_eq!(result[0].categories, ["Science"]);
    }

    #[test]
    fn rejects_wrong_digest_and_valid_but_wrongly_pinned_bytes() {
        for (structured_pin, regular_pin, bad_side) in [
            ("0".repeat(64), pin(REGULAR), InputKind::Structured),
            (pin(STRUCTURED), "0".repeat(64), InputKind::Regular),
        ] {
            assert_eq!(
                import_pinned_enterprise_ndjson_scoped(
                    Cursor::new(STRUCTURED),
                    Cursor::new(REGULAR),
                    "enwiki",
                    "20261010",
                    vec![],
                    &structured_pin,
                    &regular_pin,
                ),
                Err(PinnedImportError::DigestMismatch(bad_side))
            );
        }
        assert_eq!(
            import_pinned_enterprise_ndjson_scoped(
                Cursor::new(STRUCTURED),
                Cursor::new(REGULAR),
                "enwiki",
                "20261010",
                vec![],
                "not-a-sha256",
                &pin(REGULAR),
            ),
            Err(PinnedImportError::InvalidDigestPin)
        );

        // Trailing whitespace is parseable as JSON but still changes the
        // exact upstream bytes; the pinned receipt must catch it.
        let modified_regular = format!("{REGULAR} ");
        assert_eq!(
            import_pinned_enterprise_ndjson_scoped(
                Cursor::new(STRUCTURED),
                Cursor::new(modified_regular),
                "enwiki",
                "20261010",
                vec![],
                &pin(STRUCTURED),
                &pin(REGULAR),
            ),
            Err(PinnedImportError::DigestMismatch(InputKind::Regular))
        );
    }

    #[test]
    fn rejects_revision_mismatch_even_when_digests_match() {
        let different = REGULAR.replace(r#""identifier":17"#, r#""identifier":18"#);
        assert!(matches!(
            import_pinned_enterprise_ndjson_scoped(
                Cursor::new(STRUCTURED),
                Cursor::new(different.as_str()),
                "enwiki",
                "20261010",
                vec![],
                &pin(STRUCTURED),
                &pin(&different),
            ),
            Err(PinnedImportError::Import(_))
        ));
    }
}
