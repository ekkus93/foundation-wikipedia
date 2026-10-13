//! Construct MediaWiki Action API requests against official Wikipedia hosts.
//!
//! URL construction is not HTTP transport or endpoint authentication. The
//! eventual HTTPS client must enforce certificate validation, response size
//! limits, timeouts, redirect restrictions, and the selected offline policy.

use url::Url;
use wiki_model::ArticleKey;

const MAX_QUERY_BYTES: usize = 512;
const MAX_SEARCH_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineRequestError {
    InvalidProject,
    InvalidPageId,
    InvalidRevisionId,
    InvalidSearch,
    InvalidTitle,
    InvalidLimit,
}

/// A deliberately restricted Wikipedia endpoint. This does not accept a
/// caller-controlled host, port, scheme, credentials, or URL path.
/// Other Wikimedia projects need explicit separately reviewed mappings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikipediaRequests {
    project: String,
    api_url: String,
}

impl WikipediaRequests {
    /// Map standard <language>wiki project identifiers to official hosts.
    /// Exotic project aliases are rejected rather than guessed.
    pub fn new(project: &str) -> Result<Self, OnlineRequestError> {
        let language = project
            .strip_suffix("wiki")
            .ok_or(OnlineRequestError::InvalidProject)?;
        if !(2..=16).contains(&language.len())
            || language.starts_with('-')
            || language.ends_with('-')
            || language.contains("--")
            || !language
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(OnlineRequestError::InvalidProject);
        }
        ArticleKey {
            project: project.to_owned(),
            page_id: 1,
        }
        .validate()
        .map_err(|_| OnlineRequestError::InvalidProject)?;
        Ok(Self {
            project: project.to_owned(),
            api_url: format!("https://{language}.wikipedia.org/w/api.php"),
        })
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    fn request(&self, parameters: &[(&str, String)]) -> String {
        // The base URL is built from a validated Wikipedia language label.
        let mut url = Url::parse(&self.api_url).expect("validated Wikipedia endpoint");
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("format", "json");
            query.append_pair("formatversion", "2");
            for (key, value) in parameters {
                query.append_pair(key, value);
            }
        }
        url.into()
    }

    /// Return an encoded search URL; search snippets are untrusted and should
    /// be discarded by online_search::parse_search_results.
    pub fn search(&self, text: &str, limit: usize) -> Result<String, OnlineRequestError> {
        if text.trim().is_empty()
            || text != text.trim()
            || text.len() > MAX_QUERY_BYTES
            || text.chars().any(char::is_control)
        {
            return Err(OnlineRequestError::InvalidSearch);
        }
        if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
            return Err(OnlineRequestError::InvalidLimit);
        }
        Ok(self.request(&[
            ("action", "query".to_owned()),
            ("list", "search".to_owned()),
            ("srsearch", text.to_owned()),
            ("srnamespace", "0".to_owned()),
            ("srlimit", limit.to_string()),
        ]))
    }

    /// Request one title's stable page identity, following explicit redirect
    /// chains; caller must validate response edges with online_title.
    pub fn resolve_title(&self, title: &str) -> Result<String, OnlineRequestError> {
        if title.trim().is_empty()
            || title != title.trim()
            || title.len() > MAX_QUERY_BYTES
            || title.chars().any(char::is_control)
        {
            return Err(OnlineRequestError::InvalidTitle);
        }
        Ok(self.request(&[
            ("action", "query".to_owned()),
            ("titles", title.to_owned()),
            ("redirects", "1".to_owned()),
        ]))
    }

    /// Request the current revision of a specific stable page ID. No title
    /// redirect resolution or silent page identity substitution is requested.
    pub fn current_revision(&self, page_id: u64) -> Result<String, OnlineRequestError> {
        self.valid_page(page_id)?;
        Ok(self.request(&[
            ("action", "query".to_owned()),
            ("prop", "revisions".to_owned()),
            ("pageids", page_id.to_string()),
            ("rvprop", "ids|timestamp".to_owned()),
            ("rvlimit", "1".to_owned()),
        ]))
    }

    /// Request rendered HTML for an exact revision. Consumers MUST join the
    /// parsed page ID, revision ID and title against pinned query metadata.
    pub fn rendered_revision(&self, revision_id: u64) -> Result<String, OnlineRequestError> {
        if revision_id == 0 || revision_id > i64::MAX as u64 {
            return Err(OnlineRequestError::InvalidRevisionId);
        }
        Ok(self.request(&[
            ("action", "parse".to_owned()),
            ("oldid", revision_id.to_string()),
            ("prop", "text".to_owned()),
        ]))
    }

    fn valid_page(&self, page_id: u64) -> Result<(), OnlineRequestError> {
        if page_id > i64::MAX as u64 {
            return Err(OnlineRequestError::InvalidPageId);
        }
        ArticleKey {
            project: self.project.clone(),
            page_id,
        }
        .validate()
        .map_err(|_| OnlineRequestError::InvalidPageId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn query(url: &str) -> BTreeMap<String, String> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[test]
    fn official_origin_and_encoded_search_do_not_allow_parameter_injection() {
        let api = WikipediaRequests::new("enwiki").unwrap();
        let url = api.search("café &list=allusers", 12).unwrap();
        assert!(url.starts_with("https://en.wikipedia.org/w/api.php?"));
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert_eq!(parsed.host_str(), Some("en.wikipedia.org"));
        assert_eq!(parsed.path(), "/w/api.php");
        assert_eq!(parsed.query_pairs().count(), 7);
        let params = query(&url);
        assert_eq!(params.get("srsearch").unwrap(), "café &list=allusers");
        assert_eq!(params.get("list").unwrap(), "search");
        assert_eq!(params.get("srnamespace").unwrap(), "0");
        assert_eq!(params.get("srlimit").unwrap(), "12");
        assert_eq!(params.get("formatversion").unwrap(), "2");
        assert_eq!(api.project(), "enwiki");
    }

    #[test]
    fn official_title_lookup_is_encoded_and_requires_redirect_resolution() {
        let api = WikipediaRequests::new("enwiki").unwrap();
        let lookup = api.resolve_title("Énergie &list=allusers").unwrap();
        let params = query(&lookup);
        assert_eq!(params.get("action").unwrap(), "query");
        assert_eq!(params.get("titles").unwrap(), "Énergie &list=allusers");
        assert_eq!(params.get("redirects").unwrap(), "1");
        assert!(!params.contains_key("pageids"));
        for bad in ["", " A", "A ", "A\nB", "A\0B"] {
            assert_eq!(
                api.resolve_title(bad),
                Err(OnlineRequestError::InvalidTitle)
            );
        }
    }

    #[test]
    fn stable_page_and_exact_revision_requests_are_distinct() {
        let api = WikipediaRequests::new("simplewiki").unwrap();
        let current = query(&api.current_revision(42).unwrap());
        assert_eq!(current.get("pageids").unwrap(), "42");
        assert_eq!(current.get("rvprop").unwrap(), "ids|timestamp");
        assert_eq!(current.get("rvlimit").unwrap(), "1");
        assert_eq!(current.get("action").unwrap(), "query");
        assert!(!current.contains_key("redirects"));
        let rendered = query(&api.rendered_revision(300).unwrap());
        assert_eq!(rendered.get("oldid").unwrap(), "300");
        assert_eq!(rendered.get("action").unwrap(), "parse");
        assert_eq!(rendered.get("prop").unwrap(), "text");
        assert!(!rendered.contains_key("pageids"));
    }

    #[test]
    fn rejects_unsafe_projects_queries_and_numeric_identities() {
        for project in [
            "",
            "wiki",
            "ENwiki",
            "en.wikipedia.orgwiki",
            "en_wiki",
            "en--wiki",
            "-enwiki",
            "en-wiki",
            "enwiki\n",
            "enwiki.example",
            "enwikidata",
        ] {
            assert_eq!(
                WikipediaRequests::new(project),
                Err(OnlineRequestError::InvalidProject),
                "{project:?}"
            );
        }
        let api = WikipediaRequests::new("enwiki").unwrap();
        for search in ["", "  ", " A", "A ", "A\nB", "A\0B"] {
            assert_eq!(
                api.search(search, 1),
                Err(OnlineRequestError::InvalidSearch)
            );
        }
        assert_eq!(
            api.search(&"x".repeat(MAX_QUERY_BYTES + 1), 1),
            Err(OnlineRequestError::InvalidSearch)
        );
        assert_eq!(
            api.search("Physics", 0),
            Err(OnlineRequestError::InvalidLimit)
        );
        assert_eq!(
            api.search("Physics", 101),
            Err(OnlineRequestError::InvalidLimit)
        );
        assert_eq!(
            api.current_revision(0),
            Err(OnlineRequestError::InvalidPageId)
        );
        assert_eq!(
            api.current_revision(i64::MAX as u64 + 1),
            Err(OnlineRequestError::InvalidPageId)
        );
        assert_eq!(
            api.rendered_revision(0),
            Err(OnlineRequestError::InvalidRevisionId)
        );
        assert_eq!(
            api.rendered_revision(i64::MAX as u64 + 1),
            Err(OnlineRequestError::InvalidRevisionId)
        );
    }
}
