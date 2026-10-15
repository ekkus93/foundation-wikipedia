//! Public-API regression coverage for local digest-pinned deletion evidence.
//! These tests do not assert Wikimedia publisher authenticity or feed completeness.

use std::io::{self, BufReader, Cursor, Read};

use sha2::{Digest, Sha256};
use wiki_source::enterprise_pinned_deletions::{
    decode_pinned_deletions, DeletionPinnedImportError,
};

const RECORD: &str =
    "{\"schema_version\":1,\"project\":\"enwiki\",\"generation_id\":\"20261010\",\"page_id\":42}\n";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct FaultAfterPrefix {
    bytes: Vec<u8>,
    offset: usize,
    failed: bool,
}

impl Read for FaultAfterPrefix {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "source failed",
            ));
        }
        let count = out.len().min(self.bytes.len() - self.offset).min(7);
        out[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
        self.offset += count;
        self.failed = true;
        Ok(count)
    }
}

#[test]
fn public_reader_accepts_exact_bytes_and_case_insensitive_digest() {
    let expected = digest(RECORD.as_bytes());
    let records = decode_pinned_deletions(
        Cursor::new(RECORD),
        "enwiki",
        "20261010",
        &expected.to_ascii_uppercase(),
    )
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].page.page_id, 42);
    assert_eq!(records[0].generation_id, "20261010");
}

#[test]
fn public_reader_rejects_partial_io_even_with_pin_for_complete_feed() {
    let source = FaultAfterPrefix {
        bytes: RECORD.as_bytes().to_vec(),
        offset: 0,
        failed: false,
    };
    assert_eq!(
        decode_pinned_deletions(
            BufReader::new(source),
            "enwiki",
            "20261010",
            &digest(RECORD.as_bytes()),
        ),
        Err(DeletionPinnedImportError::Read)
    );
}

#[test]
fn public_reader_refuses_ambiguous_terminators_and_trailing_content() {
    for bytes in [
        RECORD.trim_end_matches('\n').to_owned(),
        RECORD.replace('\n', "\r\n"),
        format!("{RECORD} "),
        format!("{RECORD}\n"),
    ] {
        assert!(
            decode_pinned_deletions(
                Cursor::new(bytes.as_bytes()),
                "enwiki",
                "20261010",
                &digest(bytes.as_bytes()),
            )
            .is_err(),
            "noncanonical deletion evidence must not be accepted"
        );
    }
}

#[test]
fn public_decoder_rejects_repeated_page_ids_even_with_matching_digest() {
    let repeated = format!("{RECORD}{RECORD}");
    assert_eq!(
        decode_pinned_deletions(
            Cursor::new(repeated.as_bytes()),
            "enwiki",
            "20261010",
            &digest(repeated.as_bytes()),
        ),
        Err(DeletionPinnedImportError::DuplicateRecord)
    );
}
