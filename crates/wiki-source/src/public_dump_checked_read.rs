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

use std::io::{self, Read};

use sha2::{Digest, Sha256};

use crate::public_dump::DumpChecksum;

#[derive(Debug)]
pub enum CheckedMemberError<E> {
    InvalidLimit,
    Decode(E),
    ReadFailure,
    IncompleteConsumption,
    DigestMismatch,
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
        let count = match self.input.read(buf) {
            Ok(count) => count,
            Err(error) => {
                self.failed = true;
                return Err(error);
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
    if format!("{:x}", tracking.digest.finalize()) != member.sha256 {
        return Err(CheckedMemberError::DigestMismatch);
    }
    Ok(result)
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
}
