//! Parse the official Wikimedia current-content export SHA256SUMS inventory.
//!
//! A successfully parsed checksum list is NOT proof of authenticity or even
//! that the export is complete. Callers must obtain SHA256SUMS from the
//! expected official publication location and verify each downloaded member
//! against its declared digest before activating a snapshot.

use std::collections::BTreeSet;

const MAX_CHECKSUM_BYTES: usize = 32 * 1024 * 1024;
const MAX_MEMBERS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpChecksum {
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpChecksumError {
    EmptyInventory,
    InventoryTooLarge,
    InvalidLine(usize),
    InvalidPath(usize),
    DuplicatePath(String),
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with('/')
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'/'))
        && path.split('/').count() <= 64
        && path.split('/').all(|part| {
            !part.is_empty()
                && part.len() <= 255
                && part != "."
                && part != ".."
                && !part.ends_with('.')
                && !crate::windows_device_name(part)
        })
}

/// Parse GNU sha256sum-format lines (digest, two spaces or space-asterisk,
/// then relative member path). Paths are validated before filesystem use.
/// This parser does not fetch, decompress, or authenticate source data.
pub fn parse_sha256sums(text: &str) -> Result<Vec<DumpChecksum>, DumpChecksumError> {
    if text.is_empty() {
        return Err(DumpChecksumError::EmptyInventory);
    }
    if text.len() > MAX_CHECKSUM_BYTES {
        return Err(DumpChecksumError::InventoryTooLarge);
    }
    let mut members = Vec::new();
    let mut names = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let bytes = line.as_bytes();
        if bytes.len() < 67
            || !bytes[..64].iter().all(u8::is_ascii_hexdigit)
            || bytes[64] != b' '
            || !matches!(bytes[65], b' ' | b'*')
        {
            return Err(DumpChecksumError::InvalidLine(line_number));
        }
        let path = &line[66..];
        if !safe_relative_path(path) {
            return Err(DumpChecksumError::InvalidPath(line_number));
        }
        if !names.insert(path.to_ascii_lowercase()) {
            return Err(DumpChecksumError::DuplicatePath(path.to_owned()));
        }
        members.push(DumpChecksum {
            relative_path: path.to_owned(),
            sha256: line[..64].to_ascii_lowercase(),
        });
        if members.len() > MAX_MEMBERS {
            return Err(DumpChecksumError::InventoryTooLarge);
        }
    }
    if members.is_empty() {
        return Err(DumpChecksumError::EmptyInventory);
    }
    members.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(members)
}

fn page_partition(name: &str, prefix: &str) -> Option<(u64, u64)> {
    let range = name.strip_prefix(prefix)?.strip_suffix(".xml.bz2")?;
    let (first, last) = range.split_once('p')?;
    let start = first.parse::<u64>().ok()?;
    let end = last.parse::<u64>().ok()?;
    (start > 0 && end >= start).then_some((start, end))
}

fn legacy_page_partition(name: &str, prefix: &str) -> Option<(u64, u64)> {
    let suffix = name.strip_prefix(prefix)?;
    let (number, range) = suffix.split_once(".xml-p")?;
    if number.is_empty()
        || number.starts_with('0')
        || !number.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let (first, last) = range.strip_suffix(".bz2")?.split_once('p')?;
    let start = first.parse::<u64>().ok()?;
    let end = last.parse::<u64>().ok()?;
    (start > 0 && end >= start).then_some((start, end))
}

/// Check caller-selected completed export identity, not publisher trust.
pub fn matches_current_export(members: &[DumpChecksum], wiki: &str, date: &str) -> bool {
    let d = date.as_bytes();
    if members.is_empty()
        || wiki.is_empty()
        || !wiki
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || d.len() != 10
        || !d[..4].iter().all(u8::is_ascii_digit)
        || d[4] != b'-'
        || !d[5..7].iter().all(u8::is_ascii_digit)
        || d[7] != b'-'
        || &d[8..] != b"01"
        || (d[5] == b'0' && d[6] == b'0')
        || d[5] > b'1'
        || (d[5] == b'1' && d[6] > b'2')
    {
        return false;
    }
    let prefix = format!("{wiki}-{date}-p");
    let legacy_prefix = format!("{wiki}-{}-pages-meta-current", date.replace('-', ""));
    let mut format_kind = None;
    let mut names = BTreeSet::new();
    let mut partitions = Vec::with_capacity(members.len());
    for item in members {
        let name = item.relative_path.rsplit('/').next().unwrap_or("");
        if !safe_relative_path(&item.relative_path)
            || item.sha256.len() != 64
            || !item.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !names.insert(item.relative_path.to_ascii_lowercase())
        {
            return false;
        }
        let (partition, kind) = if let Some(range) = page_partition(name, &prefix) {
            (range, 0)
        } else if let Some(range) = legacy_page_partition(name, &legacy_prefix) {
            (range, 1)
        } else {
            return false;
        };
        if format_kind.is_some_and(|previous| previous != kind) {
            return false;
        }
        format_kind = Some(kind);
        partitions.push(partition);
    }
    // The inventory is untrusted input: the same page-ID range must not
    // appear twice (even under different subdirectories), and overlapping
    // ranges cannot be treated as one coherent export generation.
    partitions.sort_unstable();
    partitions.windows(2).all(|pair| pair[0].1 < pair[1].0)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn line(path: &str) -> String {
        format!("{}  {path}", "A".repeat(64))
    }

    #[test]
    fn parses_and_sorts_current_content_export_members() {
        let input = format!(
            "{}\n{}\n",
            line("enwiki-2026-10-01-p101p200.xml.bz2"),
            line("enwiki-2026-10-01-p1p100.xml.bz2")
        );
        let parsed = parse_sha256sums(&input).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].relative_path,
            "enwiki-2026-10-01-p101p200.xml.bz2"
        );
        assert_eq!(parsed[0].sha256, "a".repeat(64));
        assert!(parse_sha256sums(&format!("{} *nested/enwiki.xml.bz2", "a".repeat(64))).is_ok());
    }

    #[test]
    fn export_identity_rejects_fake_ranges_and_forged_member_descriptors() {
        let mut members = parse_sha256sums(&line("enwiki-2026-10-01-p1p100.xml.bz2")).unwrap();
        assert!(matches_current_export(&members, "enwiki", "2026-10-01"));
        for invalid in [
            "enwiki-2026-10-01-pwhatever.xml.bz2",
            "enwiki-2026-10-01-p100p1.xml.bz2",
            "enwiki-2026-10-01-p0p100.xml.bz2",
            "enwiki-2026-10-01-p1p2p3.xml.bz2",
        ] {
            members[0].relative_path = invalid.into();
            assert!(!matches_current_export(&members, "enwiki", "2026-10-01"));
        }
        members[0].relative_path = "enwiki-2026-10-01-p1p100.xml.bz2".into();
        members[0].sha256 = "not-a-digest".into();
        assert!(!matches_current_export(&members, "enwiki", "2026-10-01"));
        members[0].sha256 = "a".repeat(64);
        members.push(members[0].clone());
        assert!(!matches_current_export(&members, "enwiki", "2026-10-01"));
    }

    #[test]
    fn export_identity_rejects_overlapping_page_partitions() {
        let adjacent = parse_sha256sums(&format!(
            "{}\n{}",
            line("enwiki-2026-10-01-p1p100.xml.bz2"),
            line("enwiki-2026-10-01-p101p200.xml.bz2")
        ))
        .unwrap();
        assert!(matches_current_export(&adjacent, "enwiki", "2026-10-01"));

        // Page-ID gaps are not proof of an incomplete export: deleted and
        // absent pages need not have a current-content record.
        let gaps = parse_sha256sums(&format!(
            "{}\n{}",
            line("enwiki-2026-10-01-p1p100.xml.bz2"),
            line("enwiki-2026-10-01-p200p300.xml.bz2")
        ))
        .unwrap();
        assert!(matches_current_export(&gaps, "enwiki", "2026-10-01"));

        for other in [
            "enwiki-2026-10-01-p50p150.xml.bz2",
            "enwiki-2026-10-01-p100p200.xml.bz2",
            "nested/enwiki-2026-10-01-p1p100.xml.bz2",
        ] {
            let overlap = parse_sha256sums(&format!(
                "{}\n{}",
                line("enwiki-2026-10-01-p1p100.xml.bz2"),
                line(other)
            ))
            .unwrap();
            assert!(
                !matches_current_export(&overlap, "enwiki", "2026-10-01"),
                "accepted overlapping or repeated partition {other}"
            );
        }
    }

    #[test]
    fn legacy_public_current_dump_members_are_supported_without_format_mixing() {
        let first = "enwiki-20261001-pages-meta-current1.xml-p1p100.bz2";
        let second = "enwiki-20261001-pages-meta-current2.xml-p101p200.bz2";
        let legacy = parse_sha256sums(&format!("{}\n{}", line(first), line(second))).unwrap();
        assert!(matches_current_export(&legacy, "enwiki", "2026-10-01"));
        let current = "enwiki-2026-10-01-p201p300.xml.bz2";
        let mixed = parse_sha256sums(&format!("{}\n{}", line(first), line(current))).unwrap();
        assert!(!matches_current_export(&mixed, "enwiki", "2026-10-01"));
        for invalid in [
            "enwiki-20261001-pages-meta-current0.xml-p1p100.bz2",
            "enwiki-20261001-pages-meta-current01.xml-p1p100.bz2",
            "enwiki-20260901-pages-meta-current1.xml-p1p100.bz2",
            "enwiki-20261001-pages-meta-history1.xml-p1p100.bz2",
        ] {
            let items = parse_sha256sums(&line(invalid)).unwrap();
            assert!(!matches_current_export(&items, "enwiki", "2026-10-01"));
        }
    }

    #[test]
    fn rejects_unsafe_members_and_duplicate_aliases() {
        for path in [
            "../escape.xml.bz2",
            "dir/../escape.xml.bz2",
            "/absolute.xml.bz2",
            "dir//double.xml.bz2",
            "dir\\windows.xml.bz2",
            "C:drive.xml.bz2",
            "file name.xml.bz2",
            "dir/./file.xml.bz2",
            "dir./file.xml.bz2",
            "file.xml.bz2.",
            "CON.xml.bz2",
            "dir/NUL.txt",
            "COM1.xml.bz2",
        ] {
            assert_eq!(
                parse_sha256sums(&line(path)),
                Err(DumpChecksumError::InvalidPath(1)),
                "accepted unsafe path {path:?}"
            );
        }
        let input = format!("{}\n{}\n", line("a.xml.bz2"), line("A.xml.bz2"));
        assert_eq!(
            parse_sha256sums(&input),
            Err(DumpChecksumError::DuplicatePath("A.xml.bz2".into()))
        );
    }

    #[test]
    fn rejects_incomplete_or_malformed_checksum_lines() {
        assert_eq!(parse_sha256sums(""), Err(DumpChecksumError::EmptyInventory));
        for text in ["not-a-checksum", "0  file.xml.bz2"] {
            assert_eq!(
                parse_sha256sums(text),
                Err(DumpChecksumError::InvalidLine(1))
            );
        }
        assert_eq!(
            parse_sha256sums(&format!("{}  file.xml.bz2", "g".repeat(64))),
            Err(DumpChecksumError::InvalidLine(1))
        );
        assert_eq!(
            parse_sha256sums(&format!("{} file.xml.bz2", "a".repeat(64))),
            Err(DumpChecksumError::InvalidLine(1))
        );
    }
}
