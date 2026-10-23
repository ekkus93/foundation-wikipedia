//! Checked compressed-member consumption for a future portable dump decoder.
//!
//! This boundary hashes the *same bytes* handed to the decompressor/parser,
//! not a prior pathname lookup. A decoder must consume the entire compressed
//! member to EOF; returning after the first bzip2 stream or ignoring a read
//! failure cannot produce a successful receipt. The callback is responsible
//! for actual bzip2 integrity, XML structure and semantic page validation.
//!
//! The expected digest must already be pinned to an authenticated publication.
//! A successful read is NOT proof of Wikimedia origin, completeness, article
//! rendering, or required offline media. Do not publish any callback output
//! until the enclosing multi-member import and all checks have succeeded.

use std::io::{self, Read, Write};

use sha2::{Digest, Sha256};

use crate::public_dump::DumpChecksum;

#[derive(Debug)]
pub enum CheckedMemberError<E> {
    InvalidLimit,
    EmptyMember,
    Decode(E),
    ReadFailure,
    IncompleteConsumption,
    DigestMismatch,
    OutputFailure,
    OutputLimitExceeded,
}

struct HashingReader<R> {
    input: R,
    digest: Sha256,
    consumed: u64,
    max_bytes: u64,
    eof: bool,
    failed: bool,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // An empty destination is not proof of EOF.
        if buf.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "previous input failure",
            ));
        }
        let mut interrupted = 0u8;
        let count = loop {
            match self.input.read(buf) {
                Ok(count) => break count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted && interrupted < 8 => {
                    interrupted += 1;
                }
                Err(error) => {
                    self.failed = true;
                    return Err(error);
                }
            }
        };
        if count == 0 {
            self.eof = true;
            return Ok(0);
        }
        self.consumed = match self
            .consumed
            .checked_add(count as u64)
            .filter(|total| *total <= self.max_bytes)
        {
            Some(total) => total,
            None => {
                self.failed = true;
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "compressed member byte budget exceeded",
                ));
            }
        };
        self.digest.update(&buf[..count]);
        Ok(count)
    }
}

/// Invoke a decoder on a pinned compressed member and validate its exact
/// byte stream on return. The decoder MUST drain the compressed stream to EOF,
/// validate decompression and XML independently, and defer durable publication.
///
/// The callback must not retain references to the reader or publish a partial
/// page census. Its output becomes usable only if this function returns Ok.
/// A decoder that swallows an I/O/budget error still receives ReadFailure.
pub fn decode_checked_member<R, F, T, E>(
    input: R,
    member: &DumpChecksum,
    max_bytes: u64,
    decoder: F,
) -> Result<T, CheckedMemberError<E>>
where
    R: Read,
    F: FnOnce(&mut dyn Read) -> Result<T, E>,
{
    if max_bytes == 0 || max_bytes > (1_u64 << 42) {
        return Err(CheckedMemberError::InvalidLimit);
    }
    let mut tracking = HashingReader {
        input,
        digest: Sha256::new(),
        consumed: 0,
        max_bytes,
        eof: false,
        failed: false,
    };
    let result = decoder(&mut tracking).map_err(CheckedMemberError::Decode)?;
    if tracking.failed {
        return Err(CheckedMemberError::ReadFailure);
    }
    if !tracking.eof {
        return Err(CheckedMemberError::IncompleteConsumption);
    }
    if tracking.consumed == 0 {
        return Err(CheckedMemberError::EmptyMember);
    }
    if format!("{:x}", tracking.digest.finalize()) != member.sha256 {
        return Err(CheckedMemberError::DigestMismatch);
    }
    Ok(result)
}

struct BoundedOutput<W> {
    output: W,
    max_bytes: u64,
    written: u64,
    failed: bool,
    over_limit: bool,
}

impl<W: Write> Write for BoundedOutput<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "previous decoded output failure",
            ));
        }
        if (buf.len() as u64) > self.max_bytes - self.written {
            self.failed = true;
            self.over_limit = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decoded output byte budget exceeded",
            ));
        }
        match self.output.write(buf) {
            Ok(count) if count > 0 && count <= buf.len() => {
                self.written += count as u64;
                Ok(count)
            }
            Ok(_) => {
                self.failed = true;
                Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "decoded output writer made no progress",
                ))
            }
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "previous decoded output failure",
            ));
        }
        self.output.flush().inspect_err(|_| self.failed = true)
    }
}

/// Check both the pinned compressed bytes and a decoder's emitted byte budget.
///
/// The decoder must route ALL decompressed output through the supplied writer,
/// and consume the compressed reader through EOF. It must independently verify
/// the compression format and XML semantics. A swallowed output write error
/// cannot yield success. The output writer must be private/provisional: an
/// unsuccessful call may already have written bytes and MUST NOT publish them.
///
/// This is a composable safety boundary, NOT a bzip2/XML decoder or evidence
/// of upstream authenticity, completeness, or required offline media.
pub fn decode_checked_member_with_bounded_output<R, W, F, T, E>(
    input: R,
    member: &DumpChecksum,
    max_compressed_bytes: u64,
    output: W,
    max_decoded_bytes: u64,
    decoder: F,
) -> Result<(W, u64, T), CheckedMemberError<E>>
where
    R: Read,
    W: Write,
    F: FnOnce(&mut dyn Read, &mut dyn Write) -> Result<T, E>,
{
    if max_decoded_bytes == 0 || max_decoded_bytes > (1_u64 << 42) {
        return Err(CheckedMemberError::InvalidLimit);
    }
    let mut bounded = BoundedOutput {
        output,
        max_bytes: max_decoded_bytes,
        written: 0,
        failed: false,
        over_limit: false,
    };
    let result = decode_checked_member(input, member, max_compressed_bytes, |reader| {
        decoder(reader, &mut bounded)
    });
    if bounded.over_limit {
        return Err(CheckedMemberError::OutputLimitExceeded);
    }
    if bounded.failed {
        return Err(CheckedMemberError::OutputFailure);
    }
    let decoded = result?;
    bounded
        .flush()
        .map_err(|_| CheckedMemberError::OutputFailure)?;
    Ok((bounded.output, bounded.written, decoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn member(bytes: &[u8]) -> DumpChecksum {
        DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    fn consume_all(reader: &mut dyn Read) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    #[test]
    fn returns_decoded_result_only_after_full_digest_checked_eof() {
        let bytes = b"simulated compressed member";
        let result =
            decode_checked_member(Cursor::new(bytes), &member(bytes), 1024, consume_all).unwrap();
        assert_eq!(result, bytes);
    }

    #[test]
    fn empty_member_cannot_qualify_even_with_matching_empty_digest() {
        assert!(matches!(
            decode_checked_member(Cursor::new(b""), &member(b""), 1024, consume_all),
            Err(CheckedMemberError::EmptyMember)
        ));
    }

    #[test]
    fn rejects_mismatched_digest_and_underconsumed_trailing_bytes() {
        let bytes = b"member with a trailer";
        let pinned = member(b"different bytes");
        assert!(matches!(
            decode_checked_member(Cursor::new(bytes), &pinned, 1024, consume_all),
            Err(CheckedMemberError::DigestMismatch)
        ));
        assert!(matches!(
            decode_checked_member(Cursor::new(bytes), &member(bytes), 1024, |reader| {
                let mut first = [0; 6];
                reader.read_exact(&mut first)?;
                Ok::<_, io::Error>(first)
            }),
            Err(CheckedMemberError::IncompleteConsumption)
        ));
    }

    #[test]
    fn swallowed_read_errors_and_budget_overruns_never_qualify() {
        let bytes = b"more than four bytes";
        assert!(matches!(
            decode_checked_member(Cursor::new(bytes), &member(bytes), 4, |reader| {
                // A buggy decoder ignoring the read error must fail closed.
                let _ = consume_all(reader);
                Ok::<_, io::Error>(())
            }),
            Err(CheckedMemberError::ReadFailure)
        ));
        assert!(matches!(
            decode_checked_member(Cursor::new(bytes), &member(bytes), 0, consume_all),
            Err(CheckedMemberError::InvalidLimit)
        ));
        assert!(matches!(
            decode_checked_member(
                Cursor::new(bytes),
                &member(bytes),
                (1_u64 << 42) + 1,
                consume_all
            ),
            Err(CheckedMemberError::InvalidLimit)
        ));
    }

    struct FailingReader {
        first: bool,
    }

    impl Read for FailingReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                out[0] = b'x';
                Ok(1)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "synthetic I/O failure",
                ))
            }
        }
    }

    #[test]
    fn detects_swallowed_io_error_even_after_valid_prefix() {
        assert!(matches!(
            decode_checked_member(
                FailingReader { first: true },
                &member(b"x"),
                1024,
                |reader| {
                    let _ = consume_all(reader);
                    Ok::<_, io::Error>(())
                }
            ),
            Err(CheckedMemberError::ReadFailure)
        ));
    }

    #[test]
    fn reports_decoder_failure_without_claiming_member_integrity() {
        assert!(matches!(
            decode_checked_member(
                Cursor::new(b"xml invalid"),
                &member(b"xml invalid"),
                1024,
                |_reader| Err::<(), _>("XML error")
            ),
            Err(CheckedMemberError::Decode("XML error"))
        ));
    }

    #[test]
    fn bounded_decoder_returns_output_only_after_checked_compressed_eof() {
        let compressed = b"compressed fixture";
        let (output, decoded_bytes, value) = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member(compressed),
            1024,
            Vec::new(),
            64,
            |reader, writer| {
                let mut input = Vec::new();
                reader.read_to_end(&mut input)?;
                writer.write_all(b"expanded content")?;
                Ok::<_, io::Error>(input.len())
            },
        )
        .unwrap();
        assert_eq!(output, b"expanded content");
        assert_eq!(decoded_bytes, 16);
        assert_eq!(value, compressed.len());
    }

    #[test]
    fn swallowed_output_budget_error_cannot_qualify() {
        let compressed = b"compressed fixture";
        let result = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member(compressed),
            1024,
            Vec::new(),
            4,
            |reader, writer| {
                let _ = writer.write_all(b"too much decoded data");
                let mut input = Vec::new();
                reader.read_to_end(&mut input)?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(
            result,
            Err(CheckedMemberError::OutputLimitExceeded)
        ));
    }

    struct BrokenOutput;

    impl Write for BrokenOutput {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn swallowed_output_io_error_cannot_qualify() {
        let compressed = b"compressed fixture";
        let result = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member(compressed),
            1024,
            BrokenOutput,
            1024,
            |reader, writer| {
                let _ = writer.write_all(b"decoded");
                let mut input = Vec::new();
                reader.read_to_end(&mut input)?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(result, Err(CheckedMemberError::OutputFailure)));
    }

    #[test]
    fn decoded_output_does_not_override_digest_or_consumption_checks() {
        let compressed = b"compressed fixture";
        let bad_digest = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member(b"different"),
            1024,
            Vec::new(),
            1024,
            |reader, writer| {
                reader.read_to_end(&mut Vec::new())?;
                writer.write_all(b"decoded")?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(
            bad_digest,
            Err(CheckedMemberError::DigestMismatch)
        ));

        let early = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member(compressed),
            1024,
            Vec::new(),
            1024,
            |_reader, writer| {
                writer.write_all(b"decoded")?;
                Ok::<_, io::Error>(())
            },
        );
        assert!(matches!(
            early,
            Err(CheckedMemberError::IncompleteConsumption)
        ));

        let invalid = decode_checked_member_with_bounded_output(
            Cursor::new(compressed),
            &member(compressed),
            1024,
            Vec::new(),
            0,
            |_reader, _writer| Ok::<_, io::Error>(()),
        );
        assert!(matches!(invalid, Err(CheckedMemberError::InvalidLimit)));
    }

}
