//! Incremental UTF-8 and XML 1.0 character validation for provisional
//! decompressed public-dump bytes.
//!
//! This is a text safety guard, NOT an XML tokenizer, bzip2 decoder, source
//! authenticator, or proof of completeness. A caller must call finish() and
//! must never publish the provisional output until the entire import qualifies.

use std::io::{self, Write};

const MAX_WRITE_CHUNK: usize = 8192;

fn valid_xml_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r')
        || (' '..='\u{d7ff}').contains(&ch)
        || ('\u{e000}'..='\u{fffd}').contains(&ch)
        || ch >= '\u{10000}'
}

/// Validate UTF-8 even when a multi-byte scalar spans decoder writes.
///
/// Output is provisional and may contain a valid prefix after an error.
/// A failed write permanently poisons this guard; a missing finish() is not
/// a successful validation receipt. Use an outer decoded-byte budget.
pub struct XmlTextGuard<W> {
    output: W,
    pending: Vec<u8>,
    accepted: u64,
    failed: bool,
}

impl<W: Write> XmlTextGuard<W> {
    pub fn new(output: W) -> Self {
        Self {
            output,
            pending: Vec::new(),
            accepted: 0,
            failed: false,
        }
    }

    fn reject(&mut self, kind: io::ErrorKind) -> io::Error {
        self.failed = true;
        io::Error::new(kind, "invalid decoded XML text")
    }

    /// Return the output only after all UTF-8 scalars and the final flush
    /// qualify. The caller still must validate the XML document structure.
    pub fn finish(mut self) -> io::Result<(W, u64)> {
        if self.failed || !self.pending.is_empty() {
            return Err(self.reject(io::ErrorKind::InvalidData));
        }
        self.output.flush().inspect_err(|_| self.failed = true)?;
        Ok((self.output, self.accepted))
    }
}

impl<W: Write> Write for XmlTextGuard<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "previous XML text validation failure",
            ));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let count = buf.len().min(MAX_WRITE_CHUNK);
        let next = self
            .accepted
            .checked_add(count as u64)
            .ok_or_else(|| self.reject(io::ErrorKind::InvalidData))?;
        let mut joined = Vec::with_capacity(self.pending.len() + count);
        joined.extend_from_slice(&self.pending);
        joined.extend_from_slice(&buf[..count]);
        let complete = match std::str::from_utf8(&joined) {
            Ok(_) => joined.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => return Err(self.reject(io::ErrorKind::InvalidData)),
        };
        let valid = std::str::from_utf8(&joined[..complete])
            .expect("valid_up_to must identify complete UTF-8");
        if !valid.chars().all(valid_xml_char) {
            return Err(self.reject(io::ErrorKind::InvalidData));
        }
        if let Err(error) = self.output.write_all(&joined[..complete]) {
            self.failed = true;
            return Err(error);
        }
        self.pending.clear();
        self.pending.extend_from_slice(&joined[complete..]);
        self.accepted = next;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "previous XML text validation failure",
            ));
        }
        self.output.flush().inspect_err(|_| self.failed = true)
    }
}

/**
 * Validate the entire provisional decoded text stream while checking the
 * compressed member's pinned digest, EOF and both byte budgets.
 *
 * Unlike a decoder-owned XmlTextGuard, this wrapper always calls finish(),
 * so a decoder that swallows a malformed-UTF-8/XML-character write error or
 * leaves a trailing partial UTF-8 scalar cannot qualify its output.
 *
 * The decoder must actually decompress bzip2, consume compressed input to
 * EOF, and validate XML structure/page semantics independently. Success is
 * not evidence of source authenticity, completeness, or publishable output.
 */
pub fn decode_checked_member_with_xml_text<R, W, F, T, E>(
    input: R,
    member: &crate::public_dump::DumpChecksum,
    max_compressed_bytes: u64,
    output: W,
    max_decoded_bytes: u64,
    decoder: F,
) -> Result<(W, u64, T), crate::public_dump_checked_read::CheckedMemberError<E>>
where
    R: std::io::Read,
    W: Write,
    F: FnOnce(&mut dyn std::io::Read, &mut dyn Write) -> Result<T, E>,
{
    use crate::public_dump_checked_read::{
        decode_checked_member_with_bounded_output, CheckedMemberError,
    };

    let mut invalid_text = false;
    let result = decode_checked_member_with_bounded_output(
        input,
        member,
        max_compressed_bytes,
        output,
        max_decoded_bytes,
        |reader, writer| {
            let mut guard = XmlTextGuard::new(writer);
            let decoded = decoder(reader, &mut guard);
            if guard.finish().is_err() {
                invalid_text = true;
            }
            decoded
        },
    );
    match result {
        Ok(_) if invalid_text => Err(CheckedMemberError::InvalidXmlText),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn accepts_multilingual_xml_split_at_every_utf8_boundary() {
        let text = "<page>می‌روم क्‍ष 日本語 😀\t\n\r</page>";
        for split in 0..=text.len() {
            let mut guard = XmlTextGuard::new(Vec::new());
            guard.write_all(&text.as_bytes()[..split]).unwrap();
            guard.write_all(&text.as_bytes()[split..]).unwrap();
            let (output, count) = guard.finish().unwrap();
            assert_eq!(output, text.as_bytes());
            assert_eq!(count, text.len() as u64);
        }
    }

    #[test]
    fn rejects_xml_forbidden_controls_and_invalid_utf8() {
        for bytes in [
            b"<page>\0</page>".as_slice(),
            b"<page>\x01</page>",
            b"<page>\xc0\xaf</page>",
            b"<page>\xed\xa0\x80</page>",
            b"<page>\xf4\x90\x80\x80</page>",
            b"<page>\xef\xbf\xbe</page>",
        ] {
            let mut guard = XmlTextGuard::new(Vec::new());
            assert!(guard.write_all(bytes).is_err());
            assert!(guard.write_all(b"valid").is_err());
            assert!(guard.finish().is_err());
        }
    }

    #[test]
    fn rejects_truncated_multibyte_sequence_at_finish() {
        let mut guard = XmlTextGuard::new(Vec::new());
        guard.write_all(b"<page>\xe2\x82").unwrap();
        assert!(guard.finish().is_err());
    }

    struct BrokenSink;

    impl Write for BrokenSink {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn swallowed_sink_failure_permanently_poisons_guard() {
        let mut guard = XmlTextGuard::new(BrokenSink);
        assert!(guard.write_all(b"valid").is_err());
        assert!(guard.write_all(b"more").is_err());
        assert!(guard.finish().is_err());
    }

    #[test]
    fn bounded_member_callback_can_validate_decoded_xml_text() {
        use crate::public_dump::DumpChecksum;
        use crate::public_dump_checked_read::decode_checked_member_with_bounded_output;
        use sha2::{Digest, Sha256};

        let compressed = b"synthetic compressed fixture (not bzip2)";
        let member = DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(compressed)),
        };
        let (bytes, decoded, ()) = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member,
            1024,
            Vec::new(),
            128,
            |input, output| {
                let mut guard = XmlTextGuard::new(output);
                guard.write_all(b"<page>\xe6")?;
                guard.write_all(b"\x97\xa5</page>")?;
                guard.finish()?;
                io::copy(input, &mut io::sink())?;
                Ok::<_, io::Error>(())
            },
        )
        .unwrap();
        assert_eq!(bytes, "<page>日</page>".as_bytes());
        assert_eq!(decoded, bytes.len() as u64);
    }

    #[test]
    fn checked_xml_member_qualifies_multilingual_text_only_after_complete_input() {
        use crate::public_dump::DumpChecksum;
        use sha2::{Digest, Sha256};

        let compressed = b"synthetic compressed fixture, not bzip2";
        let member = DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(compressed)),
        };
        let (output, decoded_bytes, value) = decode_checked_member_with_xml_text(
            Cursor::new(compressed),
            &member,
            1024,
            Vec::new(),
            128,
            |reader, writer| {
                writer.write_all(b"<page>")?;
                writer.write_all("日本語".as_bytes())?;
                writer.write_all(b"</page>")?;
                io::copy(reader, &mut io::sink())?;
                Ok::<_, io::Error>(7)
            },
        )
        .unwrap();
        assert_eq!(output, "<page>日本語</page>".as_bytes());
        assert_eq!(decoded_bytes, output.len() as u64);
        assert_eq!(value, 7);
    }

    #[test]
    fn swallowed_xml_validation_errors_and_truncated_utf8_cannot_qualify() {
        use crate::public_dump::DumpChecksum;
        use crate::public_dump_checked_read::CheckedMemberError;
        use sha2::{Digest, Sha256};

        let compressed = b"synthetic member";
        let member = DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(compressed)),
        };
        for bad in [
            b"<page>\0</page>".as_slice(),
            b"<page>\xc0\xaf</page>",
            b"<page>\xe2\x82",
        ] {
            let result = decode_checked_member_with_xml_text(
                Cursor::new(compressed),
                &member,
                1024,
                Vec::new(),
                128,
                |reader, writer| {
                    // A decoder that ignores the writer error must still fail.
                    let _ = writer.write_all(bad);
                    io::copy(reader, &mut io::sink())?;
                    Ok::<_, io::Error>(())
                },
            );
            assert!(matches!(result, Err(CheckedMemberError::InvalidXmlText)));
        }
    }

    #[test]
    fn checked_xml_member_preserves_digest_eof_and_output_budget_failures() {
        use crate::public_dump::DumpChecksum;
        use crate::public_dump_checked_read::CheckedMemberError;
        use sha2::{Digest, Sha256};

        let compressed = b"synthetic member";
        let member = DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(compressed)),
        };
        let bad_digest = DumpChecksum {
            sha256: format!("{:x}", Sha256::digest(b"different")),
            ..member.clone()
        };
        let mismatch = decode_checked_member_with_xml_text(
            Cursor::new(compressed),
            &bad_digest,
            1024,
            Vec::new(),
            128,
            |reader, writer| {
                writer.write_all(b"<page/>")?;
                io::copy(reader, &mut io::sink())?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(mismatch, Err(CheckedMemberError::DigestMismatch)));

        let early = decode_checked_member_with_xml_text(
            Cursor::new(compressed),
            &member,
            1024,
            Vec::new(),
            128,
            |_reader, writer| {
                writer.write_all(b"<page/>")?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(
            early,
            Err(CheckedMemberError::IncompleteConsumption)
        ));

        let over_budget = decode_checked_member_with_xml_text(
            Cursor::new(compressed),
            &member,
            1024,
            Vec::new(),
            4,
            |reader, writer| {
                let _ = writer.write_all(b"<page/>");
                io::copy(reader, &mut io::sink())?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(
            over_budget,
            Err(CheckedMemberError::OutputLimitExceeded)
        ));
    }
}
