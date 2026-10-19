//! Strict bounded-memory parser for independently sourced page-ID census files.
//!
//! Input is one positive decimal page ID per LF-terminated line, sorted by the
//! downstream streaming auditor. This parser does not authenticate either file
//! or prove that a purportedly independent census is actually independent.

use std::io::{self, BufRead};

use crate::public_dump_census_stream::{
    audit_page_coverage_stream, CensusScope, StreamCoverageError,
};
use crate::public_dump_coverage::PageCoverageDelta;

const MAX_DIGITS: usize = 19;

/// Fail-closed iterator over LF-terminated decimal page IDs.
///
/// Each record uses a fixed 19-byte buffer. An oversized line, malformed
/// decimal, missing terminal LF, or reader error permanently ends iteration.
pub struct PageIdLines<R> {
    reader: R,
    done: bool,
}

impl<R: BufRead> PageIdLines<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            done: false,
        }
    }

    fn invalid(&mut self, kind: io::ErrorKind) -> Option<io::Result<u64>> {
        self.done = true;
        Some(Err(io::Error::new(kind, "invalid page-ID census record")))
    }
}

impl<R: BufRead> Iterator for PageIdLines<R> {
    type Item = io::Result<u64>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut digits = [0u8; MAX_DIGITS];
        let mut len = 0;
        let mut interrupted = 0u8;
        loop {
            let chunk = match self.reader.fill_buf() {
                Ok(chunk) => {
                    interrupted = 0;
                    chunk
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted && interrupted < 8 => {
                    interrupted += 1;
                    continue;
                }
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
            };
            if chunk.is_empty() {
                if len == 0 {
                    self.done = true;
                    return None;
                }
                return self.invalid(io::ErrorKind::UnexpectedEof);
            }
            let newline_at = chunk.iter().position(|byte| *byte == b'\n');
            let count = newline_at.unwrap_or(chunk.len());
            if count > MAX_DIGITS - len || !chunk[..count].iter().all(u8::is_ascii_digit) {
                return self.invalid(io::ErrorKind::InvalidData);
            }
            digits[len..len + count].copy_from_slice(&chunk[..count]);
            len += count;
            let consumed = count + usize::from(newline_at.is_some());
            self.reader.consume(consumed);
            if newline_at.is_some() {
                if len == 0 || (len > 1 && digits[0] == b'0') {
                    return self.invalid(io::ErrorKind::InvalidData);
                }
                let text =
                    std::str::from_utf8(&digits[..len]).expect("ASCII digits were validated above");
                let value = text
                    .parse::<u64>()
                    .ok()
                    .filter(|id| *id != 0 && *id <= i64::MAX as u64);
                return match value {
                    Some(id) => Some(Ok(id)),
                    None => self.invalid(io::ErrorKind::InvalidData),
                };
            }
        }
    }
}

/// Reconcile two strict, independently obtained, line-oriented ID streams.
/// Scope equality and matching IDs alone are not source authentication.
pub fn audit_page_coverage_text<E: BufRead, O: BufRead>(
    expected_scope: CensusScope<'_>,
    expected: E,
    observed_scope: CensusScope<'_>,
    observed: O,
) -> Result<PageCoverageDelta, StreamCoverageError> {
    audit_page_coverage_stream(
        expected_scope,
        PageIdLines::new(expected),
        observed_scope,
        PageIdLines::new(observed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn scope() -> CensusScope<'static> {
        CensusScope {
            project: "enwiki",
            generation: "2026-10-01",
            namespace: 0,
        }
    }

    struct InterruptingReader<R> {
        inner: R,
        remaining: usize,
    }

    impl<R: io::Read> io::Read for InterruptingReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl<R: BufRead> BufRead for InterruptingReader<R> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.remaining > 0 {
                self.remaining -= 1;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.inner.fill_buf()
        }

        fn consume(&mut self, amt: usize) {
            self.inner.consume(amt);
        }
    }

    #[test]
    fn retries_bounded_transient_interruptions_and_poison_persistent_errors() {
        let mut recovered = PageIdLines::new(InterruptingReader {
            inner: Cursor::new(b"1\\n42\\n"),
            remaining: 2,
        });
        assert_eq!(recovered.next().unwrap().unwrap(), 1);
        assert_eq!(recovered.next().unwrap().unwrap(), 42);
        assert!(recovered.next().is_none());

        let mut failed = PageIdLines::new(InterruptingReader {
            inner: Cursor::new(b"1\\n"),
            remaining: 9,
        });
        assert_eq!(
            failed.next().unwrap().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert!(failed.next().is_none());
    }

    #[test]
    fn reads_valid_records_across_tiny_buffers() {
        let reader = io::BufReader::with_capacity(2, Cursor::new(b"1\n42\n9223372036854775807\n"));
        let values = PageIdLines::new(reader)
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(values, [1, 42, i64::MAX as u64]);
    }

    #[test]
    fn rejects_malformed_truncated_or_unbounded_lines() {
        for bad in [
            &b"1"[..],
            b"1\n2",
            b"\n",
            b"0\n",
            b"01\n",
            b"+1\n",
            b"1\r\n",
            b"1 2\n",
            b"18446744073709551616\n",
            b"9999999999999999999999999999999999999999\n",
        ] {
            let mut lines = PageIdLines::new(Cursor::new(bad));
            while let Some(value) = lines.next() {
                if value.is_err() {
                    assert!(lines.next().is_none());
                    break;
                }
            }
            assert!(PageIdLines::new(Cursor::new(bad)).any(|line| line.is_err()));
        }
    }

    #[test]
    fn reconciles_matching_and_mismatched_line_streams() {
        let equal = audit_page_coverage_text(
            scope(),
            Cursor::new(b"1\n3\n5\n"),
            scope(),
            Cursor::new(b"1\n3\n5\n"),
        )
        .unwrap();
        assert_eq!(equal.expected_count, 3);
        assert_eq!(equal.observed_count, 3);

        let mismatch = audit_page_coverage_text(
            scope(),
            Cursor::new(b"1\n3\n5\n"),
            scope(),
            Cursor::new(b"1\n4\n5\n"),
        );
        assert!(matches!(
            mismatch,
            Err(StreamCoverageError::MissingOrUnexpected(_))
        ));
        let truncated = audit_page_coverage_text(
            scope(),
            Cursor::new(b"1\n3\n"),
            scope(),
            Cursor::new(b"1\n3"),
        );
        assert!(matches!(truncated, Err(StreamCoverageError::Read { .. })));
    }
}
