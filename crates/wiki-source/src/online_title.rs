//! Resolve one user-provided MediaWiki title to an exact canonical page ID.
//!
//! This parser consumes an explicit 'action=query&titles=...&redirects=1'
//! response from a trusted upstream transport. It does not authenticate
//! Wikimedia responses, fetch HTML, or treat a title alone as citation identity.

use serde_json::Value;
use std::collections::BTreeSet;
use wiki_model::ArticleKey;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_TITLE_BYTES: usize = 512;
const MAX_REDIRECT_HOPS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTitle {
    pub key: ArticleKey,
    pub requested: String,
    pub canonical_title: String,
    pub namespace: i32,
    /// Exact ordered normalized-title and redirect edges from the API.
    pub edges: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitleResolutionError {
    ResponseTooLarge,
    InvalidResponse,
    ApiError,
    InvalidTitle,
    InvalidIdentity,
    MissingPage,
    RedirectChain,
}

fn valid_title(value: &str) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && value.len() <= MAX_TITLE_BYTES
        && !value.chars().any(char::is_control)
}

/// Reject unmatched, cyclic or truncated redirects instead of silently
/// interpreting the requested alias as a stable article identity.
pub fn parse_title_resolution(
    body: &str,
    project: &str,
    requested_title: &str,
) -> Result<ResolvedTitle, TitleResolutionError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(TitleResolutionError::ResponseTooLarge);
    }
    if !valid_title(requested_title) {
        return Err(TitleResolutionError::InvalidTitle);
    }
    ArticleKey {
        project: project.into(),
        page_id: 1,
    }
    .validate()
    .map_err(|_| TitleResolutionError::InvalidIdentity)?;
    let response: Value =
        serde_json::from_str(body).map_err(|_| TitleResolutionError::InvalidResponse)?;
    if response.get("error").is_some() {
        return Err(TitleResolutionError::ApiError);
    }
    if response.get("continue").is_some() {
        return Err(TitleResolutionError::InvalidResponse);
    }
    let query = response
        .get("query")
        .ok_or(TitleResolutionError::InvalidResponse)?;
    let pages = query
        .get("pages")
        .and_then(Value::as_array)
        .ok_or(TitleResolutionError::InvalidResponse)?;
    if pages.len() != 1 {
        return Err(TitleResolutionError::InvalidResponse);
    }
    let page = &pages[0];
    if page.get("missing").is_some() || page.get("invalid").is_some() {
        return Err(TitleResolutionError::MissingPage);
    }
    let page_id = page
        .get("pageid")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0 && *id <= i64::MAX as u64)
        .ok_or(TitleResolutionError::InvalidIdentity)?;
    let namespace = page
        .get("ns")
        .and_then(Value::as_i64)
        .and_then(|number| i32::try_from(number).ok())
        .ok_or(TitleResolutionError::InvalidIdentity)?;
    let canonical_title = page
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| valid_title(value))
        .ok_or(TitleResolutionError::InvalidTitle)?;
    // Reject unsupported interwiki destinations instead of binding them
    // to an unrelated page ID in the requested project.
    if let Some(interwiki) = query.get("interwiki") {
        if !interwiki.as_array().is_some_and(Vec::is_empty) {
            return Err(TitleResolutionError::RedirectChain);
        }
    }
    let normalized = match query.get("normalized") {
        None => &[][..],
        Some(value) => value
            .as_array()
            .ok_or(TitleResolutionError::InvalidResponse)?
            .as_slice(),
    };
    if normalized.len() > 1 {
        return Err(TitleResolutionError::RedirectChain);
    }
    let redirects = match query.get("redirects") {
        None => &[][..],
        Some(value) => value
            .as_array()
            .ok_or(TitleResolutionError::InvalidResponse)?
            .as_slice(),
    };
    if redirects.len() > MAX_REDIRECT_HOPS {
        return Err(TitleResolutionError::RedirectChain);
    }
    let mut current = requested_title.to_owned();
    let mut visited = BTreeSet::new();
    visited.insert(current.clone());
    let mut edges = Vec::new();
    for edge in normalized.iter().chain(redirects) {
        // Preserve correctness until fragment and cross-project navigation
        // have dedicated, explicitly validated representations.
        if edge.get("tofragment").is_some() || edge.get("tointerwiki").is_some() {
            return Err(TitleResolutionError::RedirectChain);
        }
        let from = edge
            .get("from")
            .and_then(Value::as_str)
            .filter(|value| valid_title(value))
            .ok_or(TitleResolutionError::RedirectChain)?;
        let to = edge
            .get("to")
            .and_then(Value::as_str)
            .filter(|value| valid_title(value))
            .ok_or(TitleResolutionError::RedirectChain)?;
        if from != current || !visited.insert(to.to_owned()) {
            return Err(TitleResolutionError::RedirectChain);
        }
        edges.push((from.to_owned(), to.to_owned()));
        current = to.to_owned();
    }
    if current != canonical_title {
        return Err(TitleResolutionError::RedirectChain);
    }
    Ok(ResolvedTitle {
        key: ArticleKey {
            project: project.to_owned(),
            page_id,
        },
        requested: requested_title.to_owned(),
        canonical_title: canonical_title.to_owned(),
        namespace,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_title_alias_chain_resolves_to_stable_id() {
        let body = r#"{"query":{"normalized":[{"from":"physics_intro","to":"Physics intro"}],"redirects":[{"from":"Physics intro","to":"Physics"}],"pages":[{"pageid":42,"ns":0,"title":"Physics"}]}}"#;
        let resolved = parse_title_resolution(body, "enwiki", "physics_intro").unwrap();
        assert_eq!(resolved.key.page_id, 42);
        assert_eq!(resolved.canonical_title, "Physics");
        assert_eq!(resolved.edges.len(), 2);
        assert_eq!(
            parse_title_resolution(
                r#"{"query":{"pages":[{"pageid":19,"ns":0,"title":"Énergie"}]}}"#,
                "frwiki",
                "Énergie"
            )
            .unwrap()
            .key
            .page_id,
            19
        );
    }

    #[test]
    fn rejects_cycles_wrong_alias_and_missing_pages() {
        for input in [
            r#"{"query":{"redirects":[{"from":"A","to":"B"},{"from":"B","to":"A"}],"pages":[{"pageid":42,"ns":0,"title":"A"}]}}"#,
            r#"{"query":{"redirects":[{"from":"Someone else","to":"B"}],"pages":[{"pageid":42,"ns":0,"title":"B"}]}}"#,
            r#"{"query":{"redirects":[{"from":"A","to":"B"}],"pages":[{"pageid":42,"ns":0,"title":"C"}]}}"#,
        ] {
            assert_eq!(
                parse_title_resolution(input, "enwiki", "A"),
                Err(TitleResolutionError::RedirectChain)
            );
        }
        assert_eq!(
            parse_title_resolution(
                r#"{"query":{"pages":[{"missing":true,"ns":0,"title":"A"}]}}"#,
                "enwiki",
                "A"
            ),
            Err(TitleResolutionError::MissingPage)
        );
        assert_eq!(
            parse_title_resolution(r#"{"query":{"pages":[]}}"#, "enwiki", "A"),
            Err(TitleResolutionError::InvalidResponse)
        );
    }

    #[test]
    fn rejects_unsafe_identifiers_and_unbounded_responses() {
        assert_eq!(
            parse_title_resolution(r#"{"error":{"code":"badtitle"}}"#, "enwiki", "A"),
            Err(TitleResolutionError::ApiError)
        );
        let okay = r#"{"query":{"pages":[{"pageid":1,"ns":0,"title":"A"}]}}"#;
        assert_eq!(
            parse_title_resolution(okay, "enwiki", "Bad\nA"),
            Err(TitleResolutionError::InvalidTitle)
        );
        assert_eq!(
            parse_title_resolution(okay, "ENWIKI", "A"),
            Err(TitleResolutionError::InvalidIdentity)
        );
        assert_eq!(
            parse_title_resolution(&" ".repeat(MAX_RESPONSE_BYTES + 1), "enwiki", "A"),
            Err(TitleResolutionError::ResponseTooLarge)
        );
    }
}
