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
        && path.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'/')
        })
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
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
        assert_eq!(parsed[0].relative_path, "enwiki-2026-10-01-p1p100.xml.bz2");
        assert_eq!(parsed[0].sha256, "a".repeat(64));
        assert!(parse_sha256sums(&format!(
            "{} *nested/enwiki.xml.bz2",
            "a".repeat(64)
        ))
        .is_ok());
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
        for text in [
            "not-a-checksum",
            "0  file.xml.bz2",
            "g".repeat(64).as_str(),
        ] {
            assert_eq!(
                parse_sha256sums(text),
                Err(DumpChecksumError::InvalidLine(1))
            );
        }
        assert_eq!(
            parse_sha256sums(&format!("{} file.xml.bz2", "a".repeat(64))),
            Err(DumpChecksumError::InvalidLine(1))
        );
    }
}
