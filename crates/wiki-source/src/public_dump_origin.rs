//! Validate the official Wikimedia current-content checksum publication URL.
//! URL matching alone does not authenticate TLS or verify downloaded bytes.

/// Match a known release's exact official checksum inventory URL.
///
/// The public current-content feed can publish on any calendar day. The
/// URL's project and date must match the caller's pinned release identity.
pub fn is_official_checksum_url(url: &str, wiki: &str, date: &str) -> bool {
    if wiki.is_empty()
        || wiki.len() > 255
        || !wiki.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !wiki
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !wiki
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || !valid_calendar_date(date)
    {
        return false;
    }
    url == format!(
        "https://dumps.wikimedia.org/other/mediawiki_content_current/{wiki}/{date}/xml/bzip2/SHA256SUMS"
    )
}

fn valid_calendar_date(date: &str) -> bool {
    let d = date.as_bytes();
    if d.len() != 10
        || d[4] != b'-'
        || d[7] != b'-'
        || !d[..4].iter().all(u8::is_ascii_digit)
        || !d[5..7].iter().all(u8::is_ascii_digit)
        || !d[8..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let year = date[..4].parse::<u32>().unwrap_or(0);
    let month = date[5..7].parse::<u32>().unwrap_or(0);
    let day = date[8..].parse::<u32>().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && day > 0 && day <= days
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_official_inventory_identity() {
        let url = "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS";
        assert!(is_official_checksum_url(url, "enwiki", "2026-10-01"));
        assert!(!is_official_checksum_url(url, "frwiki", "2026-10-01"));
        assert!(!is_official_checksum_url(url, "enwiki", "2026-10-02"));
        assert!(!is_official_checksum_url(url, "enwiki", "2026-13-01"));
        for date in ["2026-10-14", "2024-02-29", "2000-02-29", "2026-12-31"] {
            let daily = format!(
                "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/{date}/xml/bzip2/SHA256SUMS"
            );
            assert!(is_official_checksum_url(&daily, "enwiki", date));
        }
        for date in [
            "2026-02-29",
            "1900-02-29",
            "2026-04-31",
            "2026-10-00",
            "0000-01-01",
            "2026-1-01",
        ] {
            let invalid = format!(
                "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/{date}/xml/bzip2/SHA256SUMS"
            );
            assert!(!is_official_checksum_url(&invalid, "enwiki", date));
        }
        // Official dump project identifiers start with a letter and cannot
        // end with a separator; internal separators remain valid.
        for project in ["_enwiki", "enwiki_", "123wiki", "___", "en__wiki_"] {
            let candidate = format!(
                "https://dumps.wikimedia.org/other/mediawiki_content_current/{project}/2026-10-01/xml/bzip2/SHA256SUMS"
            );
            assert!(!is_official_checksum_url(&candidate, project, "2026-10-01"));
        }
        let project = "zh_min_nanwiki";
        let candidate = format!(
            "https://dumps.wikimedia.org/other/mediawiki_content_current/{project}/2026-10-01/xml/bzip2/SHA256SUMS"
        );
        assert!(is_official_checksum_url(&candidate, project, "2026-10-01"));
        assert!(!is_official_checksum_url(url, "Enwiki", "2026-10-01"));
        assert!(!is_official_checksum_url(
            &url.replace("https:", "http:"),
            "enwiki",
            "2026-10-01"
        ));
        assert!(!is_official_checksum_url(
            &format!("{url}?download=1"),
            "enwiki",
            "2026-10-01"
        ));
    }
}
