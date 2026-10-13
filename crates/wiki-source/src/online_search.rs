//! Parse bounded MediaWiki Action API search results into navigation identities.
//!
//! This adapter consumes a caller-supplied response from an explicitly
//! selected Wikimedia endpoint. It does not fetch data, establish source
//! authenticity, render article HTML, or authorize AI evidence citations.

use std::collections::HashSet;

use serde_json::Value;
use wiki_model::ArticleKey;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 500;
const MAX_TITLE_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineSearchHit {
    pub key: ArticleKey,
    pub title: String,
    pub namespace: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineSearchError {
    ResponseTooLarge,
    InvalidResponse,
    ApiError,
    InvalidIdentity,
    DuplicatePage,
    TooManyResults,
}

/// Parse `action=query&list=search&formatversion=2&format=json` responses.
///
/// Search snippets are intentionally discarded: the API may return untrusted
/// HTML, and search metadata is never treated as revision-matched evidence.
/// The caller must pin a concrete revision before opening an article or
/// allowing any source passage into RAG.
pub fn parse_search_results(
    body: &str,
    project: &str,
    limit: usize,
) -> Result<Vec<OnlineSearchHit>, OnlineSearchError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(OnlineSearchError::ResponseTooLarge);
    }
    let project_key = ArticleKey {
        project: project.to_owned(),
        page_id: 1,
    };
    project_key
        .validate()
        .map_err(|_| OnlineSearchError::InvalidIdentity)?;

    let value: Value =
        serde_json::from_str(body).map_err(|_| OnlineSearchError::InvalidResponse)?;
    if value.get("error").is_some() {
        return Err(OnlineSearchError::ApiError);
    }
    let results = value
        .get("query")
        .and_then(|query| query.get("search"))
        .and_then(Value::as_array)
        .ok_or(OnlineSearchError::InvalidResponse)?;
    if results.len() > MAX_SEARCH_RESULTS {
        return Err(OnlineSearchError::TooManyResults);
    }

    let mut seen = HashSet::new();
    let mut hits = Vec::with_capacity(results.len().min(limit).min(100));
    for result in results {
        let page_id = result
            .get("pageid")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0 && *id <= i64::MAX as u64)
            .ok_or(OnlineSearchError::InvalidIdentity)?;
        let namespace = result
            .get("ns")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(OnlineSearchError::InvalidIdentity)?;
        let title = result
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| {
                !title.trim().is_empty()
                    && title.len() <= MAX_TITLE_BYTES
                    && !title.chars().any(char::is_control)
            })
            .ok_or(OnlineSearchError::InvalidIdentity)?;
        if !seen.insert(page_id) {
            return Err(OnlineSearchError::DuplicatePage);
        }
        if hits.len() < limit.min(100) {
            hits.push(OnlineSearchHit {
                key: ArticleKey {
                    project: project.to_owned(),
                    page_id,
                },
                title: title.to_owned(),
                namespace,
            });
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_navigation_without_rendering_untrusted_snippets() {
        let body = r#"{"batchcomplete":true,"query":{"search":[
            {"pageid":12,"ns":0,"title":"Physics","snippet":"<script>bad()</script>"},
            {"pageid":34,"ns":0,"title":"Énergie","snippet":"<b>Énergie</b>"}
        ]}}"#;
        let hits = parse_search_results(body, "enwiki", 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key.page_id, 12);
        assert_eq!(hits[0].title, "Physics");
        assert_eq!(hits[0].namespace, 0);
        assert_eq!(parse_search_results(body, "enwiki", 0).unwrap(), vec![]);
        assert_eq!(parse_search_results(body, "enwiki", 10).unwrap().len(), 2);
    }

    #[test]
    fn rejects_duplicate_and_invalid_results_even_beyond_visible_limit() {
        let duplicate = r#"{"query":{"search":[
            {"pageid":1,"ns":0,"title":"A"},
            {"pageid":1,"ns":0,"title":"B"}]}}"#;
        assert_eq!(
            parse_search_results(duplicate, "enwiki", 1),
            Err(OnlineSearchError::DuplicatePage)
        );
        for bad in [
            r#"{"query":{"search":[{"pageid":0,"ns":0,"title":"A"}]}}"#,
            r#"{"query":{"search":[{"pageid":9223372036854775808,"ns":0,"title":"A"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":2147483648,"title":"A"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":" "} ]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"A\\nB"}]}}"#,
        ] {
            assert_eq!(
                parse_search_results(bad, "enwiki", 0),
                Err(OnlineSearchError::InvalidIdentity),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_api_errors_malformed_json_and_resource_exhaustion() {
        assert_eq!(
            parse_search_results(r#"{"error":{"code":"badrequest"}}"#, "enwiki", 5),
            Err(OnlineSearchError::ApiError)
        );
        assert_eq!(
            parse_search_results("not-json", "enwiki", 5),
            Err(OnlineSearchError::InvalidResponse)
        );
        assert_eq!(
            parse_search_results(r#"{"query":{"search":[]}}"#, "ENWIKI", 5),
            Err(OnlineSearchError::InvalidIdentity)
        );
        assert_eq!(
            parse_search_results(&" ".repeat(MAX_RESPONSE_BYTES + 1), "enwiki", 5),
            Err(OnlineSearchError::ResponseTooLarge)
        );
        let many = format!(
            r#"{{"query":{{"search":[{}]}}}}"#,
            vec![r#"{"pageid":1,"ns":0,"title":"A"}"#; MAX_SEARCH_RESULTS + 1].join(",")
        );
        assert_eq!(
            parse_search_results(&many, "enwiki", 5),
            Err(OnlineSearchError::TooManyResults)
        );
    }
}
