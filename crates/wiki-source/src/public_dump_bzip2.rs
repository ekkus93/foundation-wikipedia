//! Portable, bounded bzip2 multistream decoding for provisional public dumps.
//!
//! Wikimedia public dumps can concatenate independent bzip2 streams. Use
//! MultiBzDecoder rather than the single-stream BzDecoder, and require the
//! compressed-input SHA-256, EOF, decoded-byte limit and XML envelope guards
//! to qualify together. This is NOT a publisher-authenticated complete import
//! or a MediaWiki/Parsoid renderer; the output remains provisional.

use std::io::{self, Read, Write};

use crate::public_dump::DumpChecksum;
use crate::public_dump_checked_read::CheckedMemberError;
use crate::public_dump_xml_structure::decode_checked_member_with_xml_envelope;

/// Decode every bzip2 stream in a pinned member, checking its provisional
/// XML envelope, both byte budgets, and the exact compressed digest/EOF.
///
/// The caller MUST obtain the checksum from an independently authenticated
/// official source and MUST NOT publish the returned output until all members,
/// page semantics, independent census and required media qualify.
pub fn decode_bzip2_xml_envelope<R: Read, W: Write>(
    input: R,
    member: &DumpChecksum,
    max_compressed_bytes: u64,
    output: W,
    max_decoded_bytes: u64,
) -> Result<(W, u64), CheckedMemberError<io::Error>> {
    decode_checked_member_with_xml_envelope(
        input,
        member,
        max_compressed_bytes,
        output,
        max_decoded_bytes,
        |reader, writer| {
            let mut bzip2 = bzip2::read::MultiBzDecoder::new(reader);
            io::copy(&mut bzip2, writer)?;
            Ok(())
        },
    )
    .map(|(output, decoded, ())| (output, decoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    const XML: &str = "<?xml version=\"1.0\"?><mediawiki><page><title>Physics</title></page></mediawiki>";
    const SINGLE: &str = "425a68313141592653595afe8d560000061b805001e007c0002eefdde0200054354d1b40989a681ea64da8254d43083207a8c8686f073e0c7bef3a40e75164960e83a8358d1648933c51684cd2a254443f3074c7cc5ae97442260c6e0d3f1772453850905afe8d56";
    const MULTI: &str = "425a68313141592653595e731abe000003998050016007a6afd9c02000314000d03264144c7a48f44d3d348f2992a802d6941307901675f35f22ab37246ed49dddee9f8bb9229c28482f398d5f00425a683131415926535967232ba50000119b800000800540002eee4ca02000314c989906460d26803d43d410670acc629c44ec2256cf7a0c021cf3f260fbe2ee48a70a120ce46574a0";

    fn decode_hex(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn member(bytes: &[u8]) -> DumpChecksum {
        DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    fn decode(bytes: &[u8], pinned: &DumpChecksum, limit: u64) -> Result<(Vec<u8>, u64), CheckedMemberError<io::Error>> {
        decode_bzip2_xml_envelope(
            Cursor::new(bytes),
            pinned,
            4096,
            Vec::new(),
            limit,
        )
    }

    #[test]
    fn decodes_real_bzip2_single_and_concatenated_multistreams() {
        for hex in [SINGLE, MULTI] {
            let bytes = decode_hex(hex);
            let (xml, size) = decode(&bytes, &member(&bytes), 4096).unwrap();
            assert_eq!(xml, XML.as_bytes());
            assert_eq!(size, XML.len() as u64);
        }
    }

    #[test]
    fn rejects_corruption_junk_digest_and_output_exhaustion() {
        let bytes = decode_hex(SINGLE);
        let mut corrupted = bytes.clone();
        corrupted[40] ^= 1;
        assert!(decode(&corrupted, &member(&corrupted), 4096).is_err());

        let mut appended = bytes.clone();
        appended.extend_from_slice(b"junk");
        assert!(decode(&appended, &member(&appended), 4096).is_err());

        let pinned = member(&bytes);
        assert!(matches!(
            decode(&bytes, &member(b"other"), 4096),
            Err(CheckedMemberError::DigestMismatch)
        ));
        assert!(matches!(
            decode(&bytes, &pinned, 16),
            Err(CheckedMemberError::OutputLimitExceeded)
        ));
    }
}
