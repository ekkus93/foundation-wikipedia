//! Validate the official Wikimedia current-content checksum publication URL.
//! URL matching alone does not authenticate TLS or verify downloaded bytes.

/// Match a known release's exact official checksum inventory URL.
///
/// The public current-content feed can publish on any calendar day. The
/// URL's project and date must match the caller's pinned release identity.
pub fn is_official_checksum_url(url: &str, wiki: &str, date: &str) -> bool {
    if !crate::valid_wiki_project(wiki) || !crate::public_dump::valid_dump_date(date) {
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
        for date in ["2026-10-14", "2024-02-29", "2000-02-29", "2026-12-31"] {
            let daily = format!(
                "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/{date}/xml/bzip2/SHA256SUMS"
            );
            assert!(is_official_checksum_url(&daily, "enwiki", date));
        }
        for date in [
            "2026-02-29",
            "2026-02-30",
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
