//! Bounded portable Rust reader for diagnostic revision-content JSONL.
//!
//! This validates output from the separately checksum-verified Python XML
//! diagnostic. It does not authenticate Wikimedia, decompress XML/bzip2,
//! establish an independent page census, render HTML, or qualify an offline pack.

use std::io::{self, BufRead};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::public_dump_census_stream::{
    audit_page_coverage_stream, CensusScope, StreamCoverageError,
};
use crate::public_dump_census_text::PageIdLines;
use crate::public_dump_coverage::PageCoverageDelta;

const MAX_RECORD_BYTES: usize = 128 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 32 * 1024 * 1024;
const MAX_RECORDS: u64 = 100_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticContentRevision {
    pub schema_version: u32,
    pub project: String,
    pub generation_id: String,
    pub page_id: u64,
    pub namespace: i32,
    pub title: String,
    pub revision_id: u64,
    pub timestamp: String,
    pub redirect_target: Option<String>,
    pub wikitext: String,
    pub wikitext_sha256: String,
}

#[derive(Debug)]
pub enum RevisionRecordError {
    Io(io::Error),
    InvalidScope,
    InvalidRecord,
    TruncatedRecord,
    OversizedRecord,
    InvalidOrder,
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let number = |from: usize, to: usize| -> u32 { value[from..to].parse::<u32>().unwrap_or(0) };
    let year = number(0, 4);
    let month = number(5, 7);
    let day = number(8, 10);
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day > 0 && day <= days
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && valid_date(&value[..10])
        && [11..13, 14..16, 17..19]
            .iter()
            .all(|r| bytes[r.clone()].iter().all(u8::is_ascii_digit))
        && value[11..13].parse::<u32>().is_ok_and(|n| n <= 23)
        && value[14..16].parse::<u32>().is_ok_and(|n| n <= 59)
        && value[17..19].parse::<u32>().is_ok_and(|n| n <= 59)
}

fn valid_title(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// Fail-closed, streaming adapter for *already audited* diagnostic JSONL.
/// After any parse or I/O error the iterator is poisoned and yields no more
/// records. The caller must consume to EOF before publishing downstream data.
pub struct DiagnosticRevisionLines<R> {
    reader: R,
    project: String,
    generation: String,
    namespace: i32,
    previous: u64,
    count: u64,
    done: bool,
}

impl<R: BufRead> DiagnosticRevisionLines<R> {
    pub fn new(
        reader: R,
        project: &str,
        generation: &str,
        namespace: i32,
    ) -> Result<Self, RevisionRecordError> {
        if project.is_empty()
            || project.len() > 255
            || !project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || !valid_date(generation)
            || namespace < 0
        {
            return Err(RevisionRecordError::InvalidScope);
        }
        Ok(Self {
            reader,
            project: project.into(),
            generation: generation.into(),
            namespace,
            previous: 0,
            count: 0,
            done: false,
        })
    }

    fn validate(&self, record: &DiagnosticContentRevision) -> Result<(), RevisionRecordError> {
        if record.page_id != 0 && record.page_id <= self.previous {
            return Err(RevisionRecordError::InvalidOrder);
        }
        if record.schema_version != 1
            || record.project != self.project
            || record.generation_id != self.generation
            || record.namespace != self.namespace
            || record.page_id == 0
            || record.page_id > i64::MAX as u64
            || record.revision_id == 0
            || record.revision_id > i64::MAX as u64
            || !valid_title(&record.title)
            || !valid_timestamp(&record.timestamp)
            || record
                .redirect_target
                .as_ref()
                .is_some_and(|s| !valid_title(s))
            || record.wikitext.len() > MAX_TEXT_BYTES
            || record.wikitext_sha256.len() != 64
            || format!("{:x}", Sha256::digest(record.wikitext.as_bytes())) != record.wikitext_sha256
        {
            return Err(RevisionRecordError::InvalidRecord);
        }
        if self.count >= MAX_RECORDS {
            return Err(RevisionRecordError::OversizedRecord);
        }
        Ok(())
    }
}

impl<R: BufRead> Iterator for DiagnosticRevisionLines<R> {
    type Item = Result<DiagnosticContentRevision, RevisionRecordError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut line = Vec::new();
        loop {
            let chunk = match self.reader.fill_buf() {
                Ok(chunk) => chunk,
                Err(error) => {
                    self.done = true;
                    return Some(Err(RevisionRecordError::Io(error)));
                }
            };
            if chunk.is_empty() {
                self.done = true;
                return if line.is_empty() {
                    None
                } else {
                    Some(Err(RevisionRecordError::TruncatedRecord))
                };
            }
            let newline = chunk.iter().position(|b| *b == b'\n');
            let count = newline.unwrap_or(chunk.len());
            let size = count + usize::from(newline.is_some());
            if size > MAX_RECORD_BYTES - line.len() {
                self.done = true;
                return Some(Err(RevisionRecordError::OversizedRecord));
            }
            line.extend_from_slice(&chunk[..size]);
            self.reader.consume(size);
            if newline.is_some() {
                break;
            }
        }
        // The diagnostic export contract is LF-only JSONL. A raw CR is not
        // a valid record separator and must not be accepted as JSON whitespace.
        // An escaped \r within wikitext remains valid.
        if line.contains(&b'\r') {
            self.done = true;
            return Some(Err(RevisionRecordError::InvalidRecord));
        }
        let record: DiagnosticContentRevision = match serde_json::from_slice(&line) {
            Ok(record) => record,
            Err(_) => {
                self.done = true;
                return Some(Err(RevisionRecordError::InvalidRecord));
            }
        };
        if let Err(error) = self.validate(&record) {
            self.done = true;
            return Some(Err(error));
        }
        self.previous = record.page_id;
        self.count += 1;
        Some(Ok(record))
    }
}

/// Compare page IDs from verified diagnostic revision records against an
/// independently supplied LF-delimited census of the same project, generation
/// and namespace. The caller must separately authenticate that independence.
/// A corrupt revision record, malformed expected line or different set of IDs
/// fails closed without producing a publishable completeness receipt.
#[derive(Debug)]
pub enum DiagnosticCensusError {
    Record(RevisionRecordError),
    Coverage(StreamCoverageError),
}

pub fn audit_revision_content_census<E: BufRead, O: BufRead>(
    expected: E,
    observed: O,
    scope: CensusScope<'_>,
) -> Result<PageCoverageDelta, DiagnosticCensusError> {
    let observed_records =
        DiagnosticRevisionLines::new(observed, scope.project, scope.generation, scope.namespace)
            .map_err(DiagnosticCensusError::Record)?;
    let observed_ids = observed_records.map(|item| {
        item.map(|record| record.page_id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}")))
    });
    audit_page_coverage_stream(scope, PageIdLines::new(expected), scope, observed_ids)
        .map_err(DiagnosticCensusError::Coverage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn record(page_id: u64, text: &str) -> String {
        let row = serde_json::json!({
            "schema_version": 1,
            "project": "enwiki",
            "generation_id": "2026-10-01",
            "page_id": page_id,
            "namespace": 0,
            "title": "München",
            "revision_id": page_id + 300,
            "timestamp": "2026-10-01T12:30:00Z",
            "redirect_target": null,
            "wikitext": text,
            "wikitext_sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
        });
        format!("{row}\n")
    }

    fn parse(bytes: &[u8]) -> Result<Vec<DiagnosticContentRevision>, RevisionRecordError> {
        let input = BufReader::with_capacity(3, Cursor::new(bytes));
        DiagnosticRevisionLines::new(input, "enwiki", "2026-10-01", 0)?.collect()
    }

    #[test]
    fn accepts_scoped_unicode_records_from_tiny_read_buffers() {
        let joined = record(1, "München & 日本語") + &record(42, "== Mathematics ==");
        let rows = parse(joined.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].wikitext, "München & 日本語");
        assert_eq!(rows[1].page_id, 42);
    }

    #[test]
    fn fails_closed_on_corrupt_revision_content_and_duplicate_fields() {
        let valid = record(1, "Sample");
        for bad in [
            valid.replace("enwiki", "dewiki"),
            valid.replace("\"page_id\":1", "\"page_id\":0"),
            valid.replace("\"page_id\":1", "\"page_id\":1,\"page_id\":1"),
            valid.replace("2026-10-01T12:30:00Z", "2026-02-30T12:30:00Z"),
            valid.replace("\"schema_version\":1", "\"schema_version\":2"),
            valid.replace("\"wikitext\":\"Sample\"", "\"wikitext\":\"Tampered\""),
            valid.replace("\"namespace\":0", "\"namespace\":1"),
            valid.replace("\"title\":\"München\"", "\"title\":\"Bad\\nTitle\""),
            format!("{}{}", valid, record(1, "Again")),
            valid.replace('\n', "\r\n"),
            valid.trim_end_matches('\n').into(),
        ] {
            let mut reader = DiagnosticRevisionLines::new(
                BufReader::new(Cursor::new(bad.as_bytes())),
                "enwiki",
                "2026-10-01",
                0,
            )
            .unwrap();
            assert!(reader.any(|row| row.is_err()), "failed to reject: {bad}");
        }
    }

    #[test]
    fn classifies_out_of_order_records_and_poisoned_iteration() {
        let input = record(42, "First") + &record(41, "Out of order") + &record(43, "Later");
        let mut lines = DiagnosticRevisionLines::new(
            BufReader::with_capacity(3, Cursor::new(input.as_bytes())),
            "enwiki",
            "2026-10-01",
            0,
        )
        .unwrap();
        assert_eq!(lines.next().unwrap().unwrap().page_id, 42);
        assert!(matches!(lines.next(), Some(Err(RevisionRecordError::InvalidOrder))));
        assert!(lines.next().is_none());

        let input = record(42, "First") + &record(42, "Duplicate");
        let mut lines =
            DiagnosticRevisionLines::new(Cursor::new(input.as_bytes()), "enwiki", "2026-10-01", 0)
                .unwrap();
        assert!(lines.next().unwrap().is_ok());
        assert!(matches!(
            lines.next(),
            Some(Err(RevisionRecordError::InvalidOrder))
        ));
        assert!(lines.next().is_none());
    }

    #[test]
    fn reconciles_revision_records_with_an_independently_supplied_id_stream() {
        let observed = record(1, "First") + &record(42, "Second");
        let scope = CensusScope {
            project: "enwiki",
            generation: "2026-10-01",
            namespace: 0,
        };
        let equal = audit_revision_content_census(
            Cursor::new(b"1\n42\n"),
            Cursor::new(observed.as_bytes()),
            scope,
        )
        .unwrap();
        assert_eq!((equal.expected_count, equal.observed_count), (2, 2));

        for expected in [&b"1\n41\n"[..], b"1\n", b"1\n42", b"42\n1\n"] {
            assert!(audit_revision_content_census(
                Cursor::new(expected),
                Cursor::new(observed.as_bytes()),
                scope,
            )
            .is_err());
        }
        let corrupted = observed.replace("\"wikitext\":\"Second\"", "\"wikitext\":\"Changed\"");
        assert!(audit_revision_content_census(
            Cursor::new(b"1\n42\n"),
            Cursor::new(corrupted.as_bytes()),
            scope,
        )
        .is_err());
    }

    #[test]
    fn rejects_invalid_expected_scope_and_trailing_record() {
        assert!(DiagnosticRevisionLines::new(Cursor::new(b""), "ENwiki", "2026-10-01", 0).is_err());
        assert!(DiagnosticRevisionLines::new(Cursor::new(b""), "enwiki", "2026-02-30", 0).is_err());
        assert!(
            DiagnosticRevisionLines::new(Cursor::new(b""), "enwiki", "2026-10-01", -1).is_err()
        );
        let mut lines = DiagnosticRevisionLines::new(
            Cursor::new(record(1, "Valid").as_bytes().to_vec()),
            "enwiki",
            "2026-10-01",
            0,
        )
        .unwrap();
        assert!(lines.next().unwrap().is_ok());
        assert!(lines.next().is_none());
    }
}
