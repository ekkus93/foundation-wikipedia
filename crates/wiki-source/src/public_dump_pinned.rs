//! Digest-pinned public-dump inventory. A caller-supplied digest is NOT
//! evidence of Wikimedia publisher authenticity or source completeness.
use crate::public_dump::{
    declared_member_page_range, matches_current_export, parse_sha256sums, DumpChecksum,
    DumpChecksumError,
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

    /// The canonical HTTPS checksum publication location for this validated
    /// generation. The URL is a source-location hint, not TLS verification,
    /// publisher authentication or evidence of a complete dump.
    pub fn official_inventory_url(&self) -> String {
        format!("{}/SHA256SUMS", self.official_export_base_url())
    }

    /// The official download location for exactly one checksum-declared member.
    /// A caller cannot substitute an arbitrary filename or digest. The
    /// downloaded bytes still MUST be checked against members()[index].sha256.
    pub fn official_member_url(&self, index: usize) -> Option<String> {
        let member = self.members.get(index)?;
        Some(format!(
            "{}/{}",
            self.official_export_base_url(),
            member.relative_path
        ))
    }

    fn official_export_base_url(&self) -> String {
        format!(
            "https://dumps.wikimedia.org/other/mediawiki_content_current/{}/{}/xml/bzip2",
            self.project, self.date
        )
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
    let mut members = parse_sha256sums(text).map_err(PinnedInventoryError::InvalidInventory)?;
    if !matches_current_export(&members, project, date) {
        return Err(PinnedInventoryError::WrongGeneration);
    }
    // Numbered shards sort lexically as 1, 10, 2; audit in numeric range order.
    members.sort_by_key(|member| {
        declared_member_page_range(member, project, date)
            .expect("validated generation must contain valid page ranges")
            .0
    });
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
    // When publication sidecars are staged, bind the exact SHA256SUMS bytes
    // to the already-pinned inventory. Stream with a hard byte cap instead of
    // reading the whole file: an attacker must not inflate a sidecar between
    // metadata inspection and read into an unbounded allocation. Recheck the
    // opened file identity to catch replacement by a link or another inode.
    let sidecar = root.join("SHA256SUMS");
    let prior = match std::fs::symlink_metadata(&sidecar) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(bytes),
        Err(error) => return Err(crate::public_dump_stage::StagedDumpError::Io(error)),
    };
    if !prior.file_type().is_file() {
        return Err(crate::public_dump_stage::StagedDumpError::UnsafeStagingPath);
    }
    const MAX_SIDECAR_BYTES: u64 = 32 * 1024 * 1024;
    if !(1..=MAX_SIDECAR_BYTES).contains(&prior.len()) {
        return Err(crate::public_dump_stage::StagedDumpError::InvalidInventory);
    }
    let mut file =
        std::fs::File::open(&sidecar).map_err(crate::public_dump_stage::StagedDumpError::Io)?;
    let opened = file
        .metadata()
        .map_err(crate::public_dump_stage::StagedDumpError::Io)?;
    if !opened.is_file() || opened.len() != prior.len() {
        return Err(crate::public_dump_stage::StagedDumpError::UnsafeStagingPath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if prior.dev() != opened.dev()
            || prior.ino() != opened.ino()
            || prior.nlink() != 1
            || opened.nlink() != 1
        {
            return Err(crate::public_dump_stage::StagedDumpError::UnsafeStagingPath);
        }
    }
    use std::io::Read;
    let mut digest = Sha256::new();
    let mut count = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut chunk)
            .map_err(crate::public_dump_stage::StagedDumpError::Io)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .filter(|size| *size <= MAX_SIDECAR_BYTES)
            .ok_or(crate::public_dump_stage::StagedDumpError::InvalidInventory)?;
        digest.update(&chunk[..n]);
    }
    if count != prior.len() || format!("{:x}", digest.finalize()) != inventory.inventory_sha256 {
        return Err(crate::public_dump_stage::StagedDumpError::InvalidInventory);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_member_urls_are_bound_to_digest_pinned_inventory_members() {
        let first = "enwiki-2026-10-01-p1p100.xml.bz2";
        let second = "nested/enwiki-2026-10-01-p101p200.xml.bz2";
        let raw = format!(
            "{}  {first}\n{}  {second}\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let inventory =
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let base =
            "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2";
        assert_eq!(
            inventory.official_inventory_url(),
            format!("{base}/SHA256SUMS")
        );
        assert_eq!(
            inventory.official_member_url(0).as_deref(),
            Some(format!("{base}/{first}").as_str())
        );
        assert_eq!(
            inventory.official_member_url(1).as_deref(),
            Some(format!("{base}/{second}").as_str())
        );
        assert_eq!(inventory.official_member_url(2), None);
        assert_eq!(inventory.members()[0].sha256, "a".repeat(64));
        assert_eq!(inventory.members()[1].sha256, "b".repeat(64));
    }

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
            raw.as_bytes(),
            "enwiki",
            "2026-10-01",
            url,
            &digest,
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
                    raw.as_bytes(),
                    "enwiki",
                    "2026-10-01",
                    &wrong_url,
                    &digest,
                ),
                Err(PinnedInventoryError::WrongOrigin),
            );
        }
        assert_eq!(
            parse_origin_checked_pinned_inventory(
                raw.as_bytes(),
                "enwiki",
                "2026-10-01",
                url,
                &"0".repeat(64),
            ),
            Err(PinnedInventoryError::DigestMismatch),
        );
    }

    #[cfg(unix)]
    #[test]
    fn pinned_staging_refuses_linked_checksum_sidecars() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-pinned-sidecar-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let member = "enwiki-2026-10-01-p1p100.xml.bz2";
        let payload = b"verified fixture member";
        let sums = format!("{:x}  {member}\n", Sha256::digest(payload));
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let inventory =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        std::fs::write(root.join(member), payload).unwrap();
        std::fs::write(root.join("PUBLICATION.json"), b"{}").unwrap();
        let external = root.with_extension("external");
        std::fs::write(&external, sums.as_bytes()).unwrap();

        symlink(&external, root.join("SHA256SUMS")).unwrap();
        assert!(matches!(
            verify_pinned_staged_export(&root, &inventory, 4096),
            Err(crate::public_dump_stage::StagedDumpError::UnsafeStagingPath)
        ));
        std::fs::remove_file(root.join("SHA256SUMS")).unwrap();

        std::fs::hard_link(&external, root.join("SHA256SUMS")).unwrap();
        assert!(matches!(
            verify_pinned_staged_export(&root, &inventory, 4096),
            Err(crate::public_dump_stage::StagedDumpError::UnsafeStagingPath)
        ));
        std::fs::remove_file(root.join("SHA256SUMS")).unwrap();
        std::fs::write(root.join("SHA256SUMS"), sums.as_bytes()).unwrap();
        assert_eq!(
            verify_pinned_staged_export(&root, &inventory, 4096).unwrap(),
            payload.len() as u64
        );

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_file(external).unwrap();
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
