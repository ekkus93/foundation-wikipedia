//! Streaming digest validation for verified public export inventories.
use std::io::Read;

use crate::public_dump::{parse_sha256sums, DumpChecksum};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum DumpVerifyError {
    InvalidMember,
    InvalidLimit,
    TooLarge,
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
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
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
    if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&member.sha256) {
        return Err(DumpVerifyError::DigestMismatch);
    }
    Ok(total)
}
