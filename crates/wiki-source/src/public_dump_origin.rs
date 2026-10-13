//! Validate the official Wikimedia current-content checksum publication URL.
//! URL matching alone does not authenticate TLS or verify downloaded bytes.

/// Match a known release's exact official checksum inventory URL.
pub fn is_official_checksum_url(url: &str, wiki: &str, date: &str) -> bool {
    let d = date.as_bytes();
    if wiki.is_empty()
        || wiki.len() > 255
        || !wiki
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        || d.len() != 10
        || !d[..4].iter().all(u8::is_ascii_digit)
        || d[4] != b'-'
        || !d[5..7].iter().all(u8::is_ascii_digit)
        || d[7] != b'-'
        || &d[8..] != b"01"
        || d[5..7] == *b"00"
        || d[5] > b'1'
        || (d[5] == b'1' && d[6] > b'2')
    {
        return false;
    }
    url == format!(
        "https://dumps.wikimedia.org/other/mediawiki_content_current/{wiki}/{date}/xml/bzip2/SHA256SUMS"
    )
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
