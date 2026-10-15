//! Diagnostic integrity boundary for canonical deletion-evidence NDJSON.
//!
//! This is a local versioned interchange schema, NOT an official Wikimedia
//! deletion-feed parser. Matching caller-supplied digests cannot establish
//! upstream publisher authenticity, missing deletions, or source completeness.

use std::io::{self, BufRead, BufReader, Read};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::enterprise::JoinedEnterpriseArticle;
use crate::enterprise_ndjson::MAX_BATCH_TOMBSTONES;
use crate::enterprise_pinned::{import_pinned_enterprise_ndjson_scoped, PinnedImportError};
use crate::join::{DeletionMarker, PageKey};

const MAX_DELETION_LINE_BYTES: usize = 4096;
const MAX_DELETION_TOTAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum DeletionPinnedImportError {
    InvalidDigestPin,
    InvalidRecord,
    InvalidTerminator,
    OversizedRecord,
    TooManyRecords,
    TooManyBytes,
    Read,
    DigestMismatch,
    Import(PinnedImportError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletionWireRecord {
    schema_version: u32,
    project: String,
    generation_id: String,
    page_id: u64,
}

struct DigestReader<R> {
    source: R,
    sha: Sha256,
}

impl<R: Read> Read for DigestReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes = self.source.read(buffer)?;
        self.sha.update(&buffer[..bytes]);
        Ok(bytes)
    }
}

fn valid_digest(pin: &str) -> bool {
    pin.len() == 64 && pin.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A canonical, explicitly versioned local deletion feed, not official raw
/// Wikimedia input. Every record must end in LF (CRLF is rejected), no empty
/// records or duplicates are tolerated, and the exact consumed bytes must
/// match an independently supplied digest pin.
pub fn decode_pinned_deletions<D: BufRead>(
    deletion_ndjson: D,
    project: &str,
    generation: &str,
    digest_pin: &str,
) -> Result<Vec<DeletionMarker>, DeletionPinnedImportError> {
    if !valid_digest(digest_pin) {
        return Err(DeletionPinnedImportError::InvalidDigestPin);
    }
    let mut reader = BufReader::new(DigestReader {
        source: deletion_ndjson,
        sha: Sha256::new(),
    });
    let mut markers = Vec::new();
    let mut bytes_read = 0usize;
    loop {
        let mut line = Vec::new();
        let bytes = (&mut reader)
            .take((MAX_DELETION_LINE_BYTES + 2) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| DeletionPinnedImportError::Read)?;
        if bytes == 0 {
            break;
        }
        bytes_read = bytes_read
            .checked_add(bytes)
            .ok_or(DeletionPinnedImportError::TooManyBytes)?;
        if bytes_read > MAX_DELETION_TOTAL_BYTES {
            return Err(DeletionPinnedImportError::TooManyBytes);
        }
        if bytes > MAX_DELETION_LINE_BYTES {
            return Err(DeletionPinnedImportError::OversizedRecord);
        }
        if line.last() != Some(&b'\n') || line.get(line.len().saturating_sub(2)) == Some(&b'\r') {
            return Err(DeletionPinnedImportError::InvalidTerminator);
        }
        if markers.len() >= MAX_BATCH_TOMBSTONES {
            return Err(DeletionPinnedImportError::TooManyRecords);
        }
        let record: DeletionWireRecord = serde_json::from_slice(&line[..line.len() - 1])
            .map_err(|_| DeletionPinnedImportError::InvalidRecord)?;
        if record.schema_version != 1
            || record.project != project
            || record.generation_id != generation
            || record.page_id == 0
            || record.page_id > i64::MAX as u64
        {
            return Err(DeletionPinnedImportError::InvalidRecord);
        }
        markers.push(DeletionMarker {
            page: PageKey {
                project: record.project,
                page_id: record.page_id,
            },
            generation_id: record.generation_id,
        });
    }
    let inner = reader.into_inner();
    if format!("{:x}", inner.sha.finalize()) != digest_pin.to_ascii_lowercase() {
        return Err(DeletionPinnedImportError::DigestMismatch);
    }
    Ok(markers)
}

/// Typed local import scope and independently supplied SHA-256 pins.
/// Upstream identity and digest authority must be proven by the caller.
pub struct PinnedEnterpriseInputs<'a> {
    pub project: &'a str,
    pub generation: &'a str,
    pub structured_sha256: &'a str,
    pub regular_sha256: &'a str,
    pub deletion_sha256: &'a str,
}

/// Only returns joined articles after all three independently pinned inputs
/// pass their local byte-integrity and scoped revision/deletion checks.
/// The three supplied hashes remain unauthenticated unless the caller separately
/// establishes publication origin, generation identity and feed completeness.
pub fn import_pinned_enterprise_with_deletions<S: BufRead, R: BufRead, D: BufRead>(
    structured: S,
    regular: R,
    deletion_ndjson: D,
    pins: PinnedEnterpriseInputs<'_>,
) -> Result<Vec<JoinedEnterpriseArticle>, DeletionPinnedImportError> {
    let deleted = decode_pinned_deletions(
        deletion_ndjson,
        pins.project,
        pins.generation,
        pins.deletion_sha256,
    )?;
    import_pinned_enterprise_ndjson_scoped(
        structured,
        regular,
        pins.project,
        pins.generation,
        deleted,
        pins.structured_sha256,
        pins.regular_sha256,
    )
    .map_err(DeletionPinnedImportError::Import)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const STRUCTURED: &str = r#"{"name":"Example","identifier":1,"version":{"identifier":17},"is_part_of":{"identifier":"enwiki"},"in_language":{"identifier":"en"},"namespace":{"identifier":0},"date_modified":"2026-10-10T00:00:00Z"}"#;
    const REGULAR: &str = r#"{"name":"Example","identifier":1,"version":{"identifier":17},"is_part_of":{"identifier":"enwiki"},"in_language":{"identifier":"en"},"namespace":{"identifier":0},"date_modified":"2026-10-10T00:00:00Z","article_body":{"html":"<article>Example</article>"},"categories":[{"name":"Science"}],"redirects":[]}"#;
    const DELETION: &str =
        "{\"schema_version\":1,\"project\":\"enwiki\",\"generation_id\":\"20261010\",\"page_id\":2}\n";

    fn pin(value: &str) -> String {
        format!("{:x}", Sha256::digest(value.as_bytes()))
    }

    fn import(
        deleted: &str,
        deleted_digest: &str,
    ) -> Result<Vec<JoinedEnterpriseArticle>, DeletionPinnedImportError> {
        import_pinned_enterprise_with_deletions(
            Cursor::new(STRUCTURED),
            Cursor::new(REGULAR),
            Cursor::new(deleted),
            PinnedEnterpriseInputs {
                project: "enwiki",
                generation: "20261010",
                structured_sha256: &pin(STRUCTURED),
                regular_sha256: &pin(REGULAR),
                deletion_sha256: deleted_digest,
            },
        )
    }

    #[test]
    fn joins_scoped_pinned_live_records_with_versioned_deletion_feed() {
        let joined = import(DELETION, &pin(DELETION)).unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].structured.page.page_id, 1);
        // An empty feed has a distinct valid pin but is NOT proof of deletion
        // completeness or a valid official publisher receipt.
        assert_eq!(import("", &pin("")).unwrap().len(), 1);
    }

    #[test]
    fn rejects_modified_deletion_bytes_and_wrong_pin() {
        assert_eq!(
            import(DELETION, &pin("")),
            Err(DeletionPinnedImportError::DigestMismatch)
        );
        assert_eq!(
            import(DELETION, "bad"),
            Err(DeletionPinnedImportError::InvalidDigestPin)
        );
        let modified = DELETION.replace("20261010", "20261011");
        assert_eq!(
            import(&modified, &pin(&modified)),
            Err(DeletionPinnedImportError::InvalidRecord)
        );
    }

    #[test]
    fn rejects_duplicate_deleted_pages_and_conflict_with_live_article() {
        assert!(matches!(
            import(
                &(DELETION.to_owned() + DELETION),
                &pin(&(DELETION.to_owned() + DELETION))
            ),
            Err(DeletionPinnedImportError::Import(_))
        ));
        let conflict = DELETION.replace("\"page_id\":2", "\"page_id\":1");
        assert!(matches!(
            import(&conflict, &pin(&conflict)),
            Err(DeletionPinnedImportError::Import(_))
        ));
    }

    #[test]
    fn refuses_noncanonical_or_truncated_deletion_records() {
        for bad in [
            DELETION.trim_end_matches('\n').to_owned(),
            DELETION.replace('\n', "\r\n"),
            format!("{DELETION}\n"),
            DELETION.replace("\"schema_version\":1", "\"schema_version\":2"),
            DELETION.replace("\"page_id\":2", "\"page_id\":0"),
            DELETION.replace("\"page_id\":2", "\"page_id\":2,\"page_id\":2"),
            DELETION.replace("\"page_id\":2", "\"page_id\":2,\"unexpected\":1"),
            format!("{}{}", "x".repeat(MAX_DELETION_LINE_BYTES + 1), "\n"),
        ] {
            assert!(
                import(&bad, &pin(&bad)).is_err(),
                "unexpected acceptance: {bad}"
            );
        }
    }
}
