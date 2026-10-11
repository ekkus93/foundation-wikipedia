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

fn valid_page_partition(name: &str, prefix: &str) -> bool {
    let Some(range) = name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(".xml.bz2"))
    else {
        return false;
    };
    let Some((first, last)) = range.split_once('p') else {
        return false;
    };
    match (first.parse::<u64>(), last.parse::<u64>()) {
        (Ok(start), Ok(end)) => start > 0 && end >= start,
        _ => false,
    }
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
    let mut names = BTreeSet::new();
    members.iter().all(|item| {
        let name = item.relative_path.rsplit('/').next().unwrap_or("");
        safe_relative_path(&item.relative_path)
            && item.sha256.len() == 64
            && item.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            && names.insert(item.relative_path.to_ascii_lowercase())
            && valid_page_partition(name, &prefix)
    })
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
