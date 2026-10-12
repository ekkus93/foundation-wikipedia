//! Digest-pinned public-dump inventory. A caller-supplied digest is NOT
//! evidence of Wikimedia publisher authenticity or source completeness.
use crate::public_dump::{
    matches_current_export, parse_sha256sums, DumpChecksum, DumpChecksumError,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinnedInventoryError {
    InvalidDigest,
    InvalidUtf8,
    InvalidInventory(DumpChecksumError),
    WrongGeneration,
    DigestMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedInventory {
    pub project: String,
    pub date: String,
    pub inventory_sha256: String,
    pub members: Vec<DumpChecksum>,
}

/// The expected digest must come from a separately authenticated source.
/// It covers the exact raw SHA256SUMS bytes, including their line endings.
pub fn parse_pinned_inventory(
    raw: &[u8],
    project: &str,
    date: &str,
    expected_sha256: &str,
) -> Result<PinnedInventory, PinnedInventoryError> {
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PinnedInventoryError::InvalidDigest);
    }
    let text = std::str::from_utf8(raw).map_err(|_| PinnedInventoryError::InvalidUtf8)?;
    let members = parse_sha256sums(text).map_err(PinnedInventoryError::InvalidInventory)?;
    if !matches_current_export(&members, project, date) {
        return Err(PinnedInventoryError::WrongGeneration);
    }
    let actual = format!("{:x}", Sha256::digest(raw));
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(PinnedInventoryError::DigestMismatch);
    }
    Ok(PinnedInventory {
        project: project.into(),
        date: date.into(),
        inventory_sha256: actual,
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_exact_publication_bytes_and_generation() {
        let raw = format!("{}  enwiki-2026-10-01-p1p100.xml.bz2\n", "a".repeat(64));
        let expected = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let valid =
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &expected).unwrap();
        assert_eq!(valid.members.len(), 1);
        assert_eq!(valid.inventory_sha256, expected);
        assert_eq!(
            parse_pinned_inventory(raw.as_bytes(), "frwiki", "2026-10-01", &expected),
            Err(PinnedInventoryError::WrongGeneration)
        );
        assert_eq!(
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &"0".repeat(64)),
            Err(PinnedInventoryError::DigestMismatch)
        );
        assert_eq!(
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", "bad"),
            Err(PinnedInventoryError::InvalidDigest)
        );
        assert_eq!(
            parse_pinned_inventory(&[0xff], "enwiki", "2026-10-01", &expected),
            Err(PinnedInventoryError::InvalidUtf8)
        );
    }
}
