//! Digest-pinned public-dump inventory. A caller-supplied digest is NOT
//! evidence of Wikimedia publisher authenticity or source completeness.
use crate::public_dump::{
    matches_current_export, parse_sha256sums, DumpChecksum, DumpChecksumError,
};
use crate::public_dump_origin::is_official_checksum_url;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinnedInventoryError {
    InvalidDigest,
    InvalidUtf8,
    InvalidInventory(DumpChecksumError),
    WrongGeneration,
    WrongOrigin,
    DigestMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedInventory {
    project: String,
    date: String,
    inventory_sha256: String,
    members: Vec<DumpChecksum>,
}

impl PinnedInventory {
    /// Declared checksum members in deterministic sorted order.
    pub fn members(&self) -> &[DumpChecksum] {
        &self.members
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn date(&self) -> &str {
        &self.date
    }

    pub fn inventory_sha256(&self) -> &str {
        &self.inventory_sha256
    }
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

/// Bind a digest-pinned inventory to the exact published Wikimedia current-
/// content checksum URL as well as to its declared project and generation.
///
/// A URL string match is only an origin preflight: the caller must perform
/// certificate-verified HTTPS retrieval, independently pin the expected digest,
/// and still verify each downloaded shard before activating content.
pub fn parse_origin_checked_pinned_inventory(
    raw: &[u8],
    project: &str,
    date: &str,
    checksum_url: &str,
    expected_sha256: &str,
) -> Result<PinnedInventory, PinnedInventoryError> {
    if !is_official_checksum_url(checksum_url, project, date) {
        return Err(PinnedInventoryError::WrongOrigin);
    }
    parse_pinned_inventory(raw, project, date, expected_sha256)
}

/// Stage only a member declared by the digest-pinned inventory. This does
/// not independently authenticate the caller's expected inventory digest.
pub fn stage_pinned_member<R: std::io::Read>(
    root: &std::path::Path,
    inventory: &PinnedInventory,
    member_index: usize,
    source: &mut R,
    max_bytes: u64,
) -> Result<u64, crate::public_dump_stage::StageWriteError> {
    let member = inventory
        .members
        .get(member_index)
        .ok_or(crate::public_dump_stage::StageWriteError::InvalidMember)?;
    crate::public_dump_stage::stage_verified_member(root, member, source, max_bytes)
}

/// Verify every declared member before an importer reads the staging tree.
/// The caller must keep the staging directory private against concurrent
/// untrusted filesystem mutation (path preflight is not race protection).
pub fn verify_pinned_staged_export(
    root: &std::path::Path,
    inventory: &PinnedInventory,
    max_member_bytes: u64,
) -> Result<u64, crate::public_dump_stage::StagedDumpError> {
    let bytes = crate::public_dump_stage::verify_staged_export(
        root,
        &inventory.members,
        &inventory.project,
        &inventory.date,
        max_member_bytes,
    )?;
    // When publication sidecars are staged, bind the raw SHA256SUMS bytes
    // to the already-pinned inventory digest. An arbitrary checksum sidecar
    // must not accompany otherwise verified member payloads.
    let sidecar = root.join("SHA256SUMS");
    if sidecar.exists() {
        const MAX_SIDECAR_BYTES: u64 = 32 * 1024 * 1024;
        let size = std::fs::metadata(&sidecar)
            .map_err(crate::public_dump_stage::StagedDumpError::Io)?
            .len();
        if size == 0 || size > MAX_SIDECAR_BYTES {
            return Err(crate::public_dump_stage::StagedDumpError::InvalidInventory);
        }
        let raw = std::fs::read(sidecar).map_err(crate::public_dump_stage::StagedDumpError::Io)?;
        if raw.len() as u64 != size
            || format!("{:x}", Sha256::digest(&raw)) != inventory.inventory_sha256
        {
            return Err(crate::public_dump_stage::StagedDumpError::InvalidInventory);
        }
    }
    Ok(bytes)
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

    #[test]
    fn pinned_inventory_accepts_numbered_public_current_shards() {
        let raw = format!(
            "{}  enwiki-20261001-pages-meta-current1.xml-p1p100.bz2\n{}  enwiki-20261001-pages-meta-current2.xml-p101p200.bz2\n",
            "a".repeat(64),
            "b".repeat(64),
        );
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let inventory =
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        assert_eq!(inventory.members().len(), 2);
        assert_eq!(inventory.project(), "enwiki");
        assert_eq!(inventory.date(), "2026-10-01");
    }

    #[test]
    fn official_origin_preflight_binds_pinned_inventory_to_publisher_url() {
        let raw = format!("{}  enwiki-2026-10-01-p1p100.xml.bz2\n", "a".repeat(64));
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let url = "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS";
        let valid = parse_origin_checked_pinned_inventory(
            raw.as_bytes(), "enwiki", "2026-10-01", url, &digest,
        )
        .unwrap();
        assert_eq!(valid.members().len(), 1);
        for wrong_url in [
            url.replace("https://", "http://"),
            url.replace("dumps.wikimedia.org", "dumps.wikimedia.org.evil.test"),
            format!("{url}?raw=1"),
            url.replace("/enwiki/", "/frwiki/"),
            url.replace("/2026-10-01/", "/2026-10-02/"),
        ] {
            assert_eq!(
                parse_origin_checked_pinned_inventory(
                    raw.as_bytes(), "enwiki", "2026-10-01", &wrong_url, &digest,
                ),
                Err(PinnedInventoryError::WrongOrigin),
            );
        }
        assert_eq!(
            parse_origin_checked_pinned_inventory(
                raw.as_bytes(), "enwiki", "2026-10-01", url, &"0".repeat(64),
            ),
            Err(PinnedInventoryError::DigestMismatch),
        );
    }

    #[test]
    fn pinned_staging_rejects_undeclared_members_and_corrupt_bytes() {
        use std::io::Cursor;
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("wiki-pinned-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let filename = "enwiki-2026-10-01-p1p100.xml.bz2";
        let bytes = b"fixture shard";
        let raw = format!("{:x}  {filename}\n", Sha256::digest(bytes));
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let inventory =
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();

        assert!(matches!(
            stage_pinned_member(&root, &inventory, 1, &mut Cursor::new(bytes), 1024),
            Err(crate::public_dump_stage::StageWriteError::InvalidMember)
        ));
        assert!(matches!(
            stage_pinned_member(&root, &inventory, 0, &mut Cursor::new(b"bad"), 1024),
            Err(crate::public_dump_stage::StageWriteError::DigestMismatch)
        ));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        assert_eq!(
            stage_pinned_member(&root, &inventory, 0, &mut Cursor::new(bytes), 1024).unwrap(),
            bytes.len() as u64
        );
        assert_eq!(
            verify_pinned_staged_export(&root, &inventory, 1024).unwrap(),
            bytes.len() as u64
        );
        std::fs::write(root.join("SHA256SUMS"), raw.as_bytes()).unwrap();
        std::fs::write(root.join("PUBLICATION.json"), b"{}").unwrap();
        assert_eq!(
            verify_pinned_staged_export(&root, &inventory, 1024).unwrap(),
            bytes.len() as u64
        );
        std::fs::write(root.join("SHA256SUMS"), b"forged inventory").unwrap();
        assert!(matches!(
            verify_pinned_staged_export(&root, &inventory, 1024),
            Err(crate::public_dump_stage::StagedDumpError::InvalidInventory)
        ));
        std::fs::write(root.join("SHA256SUMS"), raw.as_bytes()).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(root.join("SHA256SUMS"))
            .unwrap()
            .set_len(32 * 1024 * 1024 + 1)
            .unwrap();
        assert!(matches!(
            verify_pinned_staged_export(&root, &inventory, 1024),
            Err(crate::public_dump_stage::StagedDumpError::InvalidInventory)
        ));
        std::fs::write(root.join("SHA256SUMS"), raw.as_bytes()).unwrap();
        std::fs::write(root.join(filename), b"tampered").unwrap();
        assert!(matches!(
            verify_pinned_staged_export(&root, &inventory, 1024),
            Err(crate::public_dump_stage::StagedDumpError::Member(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
