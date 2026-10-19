//! Streaming digest validation for verified public export inventories.
use std::io::Read;

use crate::public_dump::{parse_sha256sums, DumpChecksum};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum DumpVerifyError {
    InvalidMember,
    InvalidLimit,
    TooLarge,
    EmptyMember,
    DigestMismatch,
    Io(std::io::Error),
}

pub fn verify_dump_member<R: Read>(
    reader: &mut R,
    member: &DumpChecksum,
    max_bytes: u64,
) -> Result<u64, DumpVerifyError> {
    let line = format!("{}  {}", member.sha256, member.relative_path);
    if parse_sha256sums(&line).is_err() {
        return Err(DumpVerifyError::InvalidMember);
    }
    if max_bytes == 0 || max_bytes > (1u64 << 42) {
        return Err(DumpVerifyError::InvalidLimit);
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    let mut interrupted = 0u8;
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(n) => {
                interrupted = 0;
                n
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted && interrupted < 8 => {
                interrupted += 1;
                continue;
            }
            Err(e) => return Err(DumpVerifyError::Io(e)),
        };
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(DumpVerifyError::TooLarge)?;
        if total > max_bytes {
            return Err(DumpVerifyError::TooLarge);
        }
        hash.update(&chunk[..n]);
    }
    if total == 0 {
        return Err(DumpVerifyError::EmptyMember);
    }
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&member.sha256) {
        return Err(DumpVerifyError::DigestMismatch);
    }
    Ok(total)
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

    #[test]
    fn rejects_empty_member_even_with_matching_empty_digest() {
        let mut input = Cursor::new(Vec::<u8>::new());
        assert!(matches!(
            verify_dump_member(&mut input, &member(b""), 1024),
            Err(DumpVerifyError::EmptyMember)
        ));
    }

    struct InterruptingReader<'a> {
        inner: Cursor<&'a [u8]>,
        remaining: usize,
    }

    impl std::io::Read for InterruptingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining > 0 {
                self.remaining -= 1;
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            self.inner.read(buffer)
        }
    }

    #[test]
    fn bounds_interrupted_reads_without_losing_transient_recovery() {
        let bytes = b"verified dump bytes";
        let checksum = member(bytes);
        let mut transient = InterruptingReader {
            inner: Cursor::new(bytes.as_slice()),
            remaining: 2,
        };
        assert_eq!(
            verify_dump_member(&mut transient, &checksum, 1024).unwrap(),
            bytes.len() as u64
        );
        let mut persistent = InterruptingReader {
            inner: Cursor::new(bytes.as_slice()),
            remaining: 9,
        };
        assert!(matches!(
            verify_dump_member(&mut persistent, &checksum, 1024),
            Err(DumpVerifyError::Io(error))
                if error.kind() == std::io::ErrorKind::Interrupted
        ));
    }

    struct IntermittentChunks<'a> {
        bytes: &'a [u8],
        offset: usize,
        interrupts: usize,
        reset_after_first: bool,
    }

    impl Read for IntermittentChunks<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.interrupts > 0 {
                self.interrupts -= 1;
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            if self.offset == self.bytes.len() {
                return Ok(0);
            }
            let n = (self.bytes.len() - self.offset).min(buffer.len()).min(4096);
            buffer[..n].copy_from_slice(&self.bytes[self.offset..self.offset + n]);
            self.offset += n;
            if !self.reset_after_first {
                self.reset_after_first = true;
                self.interrupts = 8;
            }
            Ok(n)
        }
    }

    #[test]
    fn verifies_multichunk_member_after_two_full_transient_retry_windows() {
        let bytes = vec![0x5a; 128 * 1024 + 3];
        let mut reader = IntermittentChunks {
            bytes: &bytes,
            offset: 0,
            interrupts: 8,
            reset_after_first: false,
        };
        assert_eq!(
            verify_dump_member(&mut reader, &member(&bytes), 200_000).unwrap(),
            bytes.len() as u64
        );
    }

    #[test]
    fn streams_nonempty_member_and_rejects_tampering_and_budget() {
        let content = b"verified dump bytes";
        let descriptor = member(content);
        let mut input = Cursor::new(content);
        assert_eq!(
            verify_dump_member(&mut input, &descriptor, 1024).unwrap(),
            content.len() as u64
        );
        let mut corrupted = Cursor::new(b"corrupted dump bytes");
        assert!(matches!(
            verify_dump_member(&mut corrupted, &descriptor, 1024),
            Err(DumpVerifyError::DigestMismatch)
        ));
        let mut input = Cursor::new(content);
        assert!(matches!(
            verify_dump_member(&mut input, &descriptor, 4),
            Err(DumpVerifyError::TooLarge)
        ));
    }
}
