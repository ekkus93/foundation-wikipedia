use std::io::Cursor;

use sha2::{Digest, Sha256};
use wiki_source::public_dump::DumpChecksum;
use wiki_source::public_dump_verify::verify_dump_member;

#[test]
fn verifies_complete_stream_digest() {
    let bytes = b"Wikipedia current XML";
    let member = DumpChecksum {
        relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    };
    assert_eq!(
        verify_dump_member(&mut Cursor::new(bytes), &member, 100).unwrap(),
        bytes.len() as u64
    );
}

#[test]
fn wrong_digest_is_rejected() {
    use wiki_source::public_dump_verify::DumpVerifyError;
    let member = DumpChecksum {
        relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
        sha256: format!("{:x}", Sha256::digest(b"original")),
    };
    assert!(matches!(
        verify_dump_member(&mut Cursor::new(b"changed"), &member, 100),
        Err(DumpVerifyError::DigestMismatch)
    ));
}
