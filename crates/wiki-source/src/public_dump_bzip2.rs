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


/// Stream page and revision identities from a pinned bzip2 member.
///
/// The callback receives provisional records before the final member digest
/// and EOF have been checked. It MUST only stage records, never publish them.
/// The returned count is not an independently authenticated page census.
pub fn decode_bzip2_xml_page_metadata<R: Read, F>(
    input: R,
    member: &DumpChecksum,
    max_compressed_bytes: u64,
    max_decoded_bytes: u64,
    on_page: F,
) -> Result<(u64, u64), CheckedMemberError<io::Error>>
where
    F: FnMut(crate::public_dump_page_meta::PublicDumpPage) -> io::Result<()>,
{
    let output = crate::public_dump_xml_events::XmlPageMetadataWriter::new(on_page);
    let (output, decoded) = decode_bzip2_xml_envelope(
        input,
        member,
        max_compressed_bytes,
        output,
        max_decoded_bytes,
    )?;
    let (_, pages) = output
        .finish()
        .map_err(|_| CheckedMemberError::InvalidXmlDocument)?;
    Ok((decoded, pages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;

    const XML: &str =
        "<?xml version=\"1.0\"?><mediawiki><page><title>Physics</title></page></mediawiki>";
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

    fn decode(
        bytes: &[u8],
        pinned: &DumpChecksum,
        limit: u64,
    ) -> Result<(Vec<u8>, u64), CheckedMemberError<io::Error>> {
        decode_bzip2_xml_envelope(Cursor::new(bytes), pinned, 4096, Vec::new(), limit)
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
    fn decodes_real_bzip2_page_metadata_and_ignores_contributor_ids() {
        let xml = "<?xml version='1.0'?><mediawiki><page><title>Physics &amp; 日本語</title><ns>0</ns><id>42</id><revision><id>900</id><timestamp>2026-10-01T12:00:00Z</timestamp><contributor><id>12345</id></contributor><text><![CDATA[{{x}}]]></text></revision></page></mediawiki>";
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let bytes = encoder.finish().unwrap();
        let mut pages = Vec::new();
        let (decoded, count) = decode_bzip2_xml_page_metadata(
            Cursor::new(&bytes),
            &member(&bytes),
            4096,
            4096,
            |page| {
                pages.push(page);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(decoded, xml.len() as u64);
        assert_eq!(count, 1);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_id, 42);
        assert_eq!(pages[0].revision_id, 900);
        assert_eq!(pages[0].title, "Physics & 日本語");
    }

    #[test]
    fn rejects_semantically_incomplete_bzip2_pages() {
        let xml = "<mediawiki><page><title>X</title><ns>0</ns><revision><id>2</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>";
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let bytes = encoder.finish().unwrap();
        assert!(decode_bzip2_xml_page_metadata(
            Cursor::new(&bytes),
            &member(&bytes),
            4096,
            4096,
            |_| Ok(()),
        )
        .is_err());
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
