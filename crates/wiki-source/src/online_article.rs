//! Fail-closed joins for MediaWiki Action API revision metadata and rendered HTML.
//!
//! Transport must fetch revision metadata from the selected official wiki and
//! render that exact oldid. Parsing these two responses does not authenticate
//! the endpoint, acquire mandatory media, or authorize AI citation evidence.

use ammonia::Builder;
use serde_json::Value;
use wiki_model::ArticleKey;

const MAX_METADATA_BYTES: usize = 1024 * 1024;
const MAX_RENDERED_BYTES: usize = 8 * 1024 * 1024;
const MAX_TITLE_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineRevision {
    pub key: ArticleKey,
    pub title: String,
    pub namespace: i32,
    pub revision_id: u64,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineRenderedRevision {
    pub revision: OnlineRevision,
    /// Sanitized HTML only. Mandatory media and embedded resources are NOT
    /// authenticated, stored, or offline-qualified by this adapter.
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineArticleError {
    ResponseTooLarge,
    InvalidResponse,
    ApiError,
    InvalidIdentity,
    MissingPage,
    RedirectNeedsResolution,
    RevisionMismatch,
    UnsafeHtml,
}

fn parse_response(body: &str, bound: usize) -> Result<Value, OnlineArticleError> {
    if body.len() > bound {
        return Err(OnlineArticleError::ResponseTooLarge);
    }
    let value: Value =
        serde_json::from_str(body).map_err(|_| OnlineArticleError::InvalidResponse)?;
    if !value.is_object() {
        return Err(OnlineArticleError::InvalidResponse);
    }
    if value.get("error").is_some() {
        return Err(OnlineArticleError::ApiError);
    }
    if value.get("continue").is_some()
        || value
            .get("batchcomplete")
            .is_some_and(|flag| flag.as_bool() != Some(true))
    {
        // A partial/multi-page response is not sufficient for revision pinning.
        return Err(OnlineArticleError::InvalidResponse);
    }
    Ok(value)
}

fn valid_title(title: &str) -> bool {
    !title.trim().is_empty()
        && title == title.trim()
        && title.len() <= MAX_TITLE_BYTES
        && !title.chars().any(char::is_control)
}

pub(crate) fn valid_utc_timestamp(stamp: &str) -> bool {
    let b = stamp.as_bytes();
    // MediaWiki rvprop=timestamp returns ISO 8601 whole-second UTC.
    // This checks the syntax and coarse ranges; it does not certify a source clock.
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || c.is_ascii_digit())
    {
        return false;
    }
    let number = |start: usize, end: usize| stamp[start..end].parse::<u32>().unwrap_or(0);
    let year = number(0, 4);
    let month = number(5, 7);
    let day = number(8, 10);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year > 0
        && (1..=days).contains(&day)
        && number(11, 13) <= 23
        && number(14, 16) <= 59
        && number(17, 19) <= 59
}

/// Validate a single page's latest revision returned by
/// 'action=query&prop=revisions&rvprop=ids|timestamp&rvlimit=1&
/// pageids=<id>&formatversion=2&format=json'.
///
/// The requested page is an identity precondition, not a title-based lookup.
/// Redirect pages require a separate explicit resolution step; never silently
/// bind an alias page's revision to an unrelated canonical article.
pub fn parse_current_revision(
    body: &str,
    project: &str,
    requested_page_id: u64,
) -> Result<OnlineRevision, OnlineArticleError> {
    let key = ArticleKey {
        project: project.to_owned(),
        page_id: requested_page_id,
    };
    key.validate()
        .map_err(|_| OnlineArticleError::InvalidIdentity)?;
    if requested_page_id > i64::MAX as u64 {
        return Err(OnlineArticleError::InvalidIdentity);
    }
    let value = parse_response(body, MAX_METADATA_BYTES)?;
    // A normalized redirect alias is not proof that the requested page ID
    // identifies the returned article. Resolve redirects explicitly before
    // accepting a revision-pinned response.
    if value
        .get("query")
        .and_then(|q| q.get("redirects"))
        .is_some()
    {
        return Err(OnlineArticleError::RedirectNeedsResolution);
    }
    let pages = value
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(Value::as_array)
        .ok_or(OnlineArticleError::InvalidResponse)?;
    if pages.len() != 1 {
        return Err(OnlineArticleError::InvalidResponse);
    }
    let page = &pages[0];
    if page.get("missing").is_some() || page.get("invalid").is_some() {
        return Err(OnlineArticleError::MissingPage);
    }
    if page.get("redirect").is_some() {
        return Err(OnlineArticleError::RedirectNeedsResolution);
    }
    if page.get("pageid").and_then(Value::as_u64) != Some(requested_page_id) {
        return Err(OnlineArticleError::InvalidIdentity);
    }
    let title = page
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| valid_title(title))
        .ok_or(OnlineArticleError::InvalidIdentity)?;
    let namespace = page
        .get("ns")
        .and_then(Value::as_i64)
        .and_then(|ns| i32::try_from(ns).ok())
        .ok_or(OnlineArticleError::InvalidIdentity)?;
    // This reader supports mainspace articles only. Enforce it at the parser
    // boundary as well as in the higher-level navigation flow.
    if namespace != 0 {
        return Err(OnlineArticleError::InvalidIdentity);
    }
    let revisions = page
        .get("revisions")
        .and_then(Value::as_array)
        .ok_or(OnlineArticleError::InvalidResponse)?;
    if revisions.len() != 1 {
        return Err(OnlineArticleError::InvalidResponse);
    }
    let revision_id = revisions[0]
        .get("revid")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0 && *id <= i64::MAX as u64)
        .ok_or(OnlineArticleError::InvalidIdentity)?;
    let timestamp = revisions[0]
        .get("timestamp")
        .and_then(Value::as_str)
        .filter(|t| valid_utc_timestamp(t))
        .ok_or(OnlineArticleError::InvalidIdentity)?;
    Ok(OnlineRevision {
        key,
        title: title.to_owned(),
        namespace,
        revision_id,
        timestamp: timestamp.to_owned(),
    })
}

/// Accept 'action=parse&oldid=<revision_id>&prop=text&formatversion=2'
/// only when its page, title, and revision agree with pinned query metadata.
/// Fail closed on missing IDs instead of treating renderer content as current.
/// Script/event-handler URLs are removed; the caller must still enforce a
/// restrictive WebView origin/link policy and verify every required visual.
pub fn parse_rendered_revision(
    body: &str,
    pinned: &OnlineRevision,
) -> Result<OnlineRenderedRevision, OnlineArticleError> {
    pinned
        .key
        .validate()
        .map_err(|_| OnlineArticleError::InvalidIdentity)?;
    if pinned.key.page_id > i64::MAX as u64
        || pinned.revision_id == 0
        || pinned.revision_id > i64::MAX as u64
        || pinned.namespace != 0
        || !valid_title(&pinned.title)
        || !valid_utc_timestamp(&pinned.timestamp)
    {
        return Err(OnlineArticleError::InvalidIdentity);
    }
    let value = parse_response(body, MAX_RENDERED_BYTES)?;
    let parsed = value
        .get("parse")
        .ok_or(OnlineArticleError::InvalidResponse)?;
    if parsed.get("pageid").and_then(Value::as_u64) != Some(pinned.key.page_id)
        || parsed.get("revid").and_then(Value::as_u64) != Some(pinned.revision_id)
        || parsed.get("title").and_then(Value::as_str) != Some(pinned.title.as_str())
    {
        return Err(OnlineArticleError::RevisionMismatch);
    }
    let raw_html = parsed
        .get("text")
        .and_then(Value::as_str)
        .filter(|html| !html.trim().is_empty())
        .ok_or(OnlineArticleError::InvalidResponse)?;
    let html = Builder::default().clean(raw_html).to_string();
    if html.trim().is_empty() {
        return Err(OnlineArticleError::UnsafeHtml);
    }
    Ok(OnlineRenderedRevision {
        revision: pinned.clone(),
        html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Énergie","revisions":[{"revid":300,"timestamp":"2026-10-13T10:45:00Z"}]}]}}"#;
    const RENDER: &str = r#"{"parse":{"title":"Énergie","pageid":42,"revid":300,"text":"<p onclick=\"bad()\">Hello</p><script>bad()</script><a href=\"javascript:bad()\">bad link</a>"}}"#;

    #[test]
    fn rejects_explicitly_incomplete_revision_responses() {
        let incomplete = META.replacen(r#"{"query":"#, r#"{"batchcomplete":false,"query":"#, 1);
        assert_eq!(
            parse_current_revision(&incomplete, "frwiki", 42),
            Err(OnlineArticleError::InvalidResponse)
        );
        let revision = parse_current_revision(META, "frwiki", 42).unwrap();
        let incomplete = RENDER.replacen(r#"{"parse":"#, r#"{"batchcomplete":false,"parse":"#, 1);
        assert_eq!(
            parse_rendered_revision(&incomplete, &revision),
            Err(OnlineArticleError::InvalidResponse)
        );
    }

    #[test]
    fn revision_timestamp_rejects_impossible_calendar_dates() {
        for bad in [
            "0000-01-01T00:00:00Z",
            "2026-02-29T10:45:00Z",
            "2026-04-31T10:45:00Z",
            "1900-02-29T10:45:00Z",
            "2026-00-01T10:45:00Z",
            "2026-01-00T10:45:00Z",
            "2026-01-01T24:00:00Z",
        ] {
            let payload = META.replace("2026-10-13T10:45:00Z", bad);
            assert_eq!(
                parse_current_revision(&payload, "frwiki", 42),
                Err(OnlineArticleError::InvalidIdentity),
                "{bad}"
            );
        }
        for valid in [
            "2024-02-29T23:59:59Z",
            "2000-02-29T00:00:00Z",
            "2026-04-30T10:45:00Z",
        ] {
            let payload = META.replace("2026-10-13T10:45:00Z", valid);
            assert!(
                parse_current_revision(&payload, "frwiki", 42).is_ok(),
                "{valid}"
            );
        }
    }

    #[test]
    fn exact_revision_join_sanitizes_untrusted_html() {
        let revision = parse_current_revision(META, "frwiki", 42).unwrap();
        assert_eq!(revision.key.page_id, 42);
        assert_eq!(revision.title, "Énergie");
        assert_eq!(revision.revision_id, 300);
        let article = parse_rendered_revision(RENDER, &revision).unwrap();
        assert!(article.html.contains("Hello"));
        for forbidden in ["<script", "onclick", "javascript:", "bad()"] {
            assert!(!article.html.contains(forbidden), "{forbidden}");
        }
        assert_eq!(article.revision, revision);
    }

    #[test]
    fn rejects_mismatched_identity_and_stale_html() {
        let revision = parse_current_revision(META, "frwiki", 42).unwrap();
        assert_eq!(
            parse_current_revision(META, "frwiki", 43),
            Err(OnlineArticleError::InvalidIdentity)
        );
        for bad in [
            RENDER.replace(r#""revid":300"#, r#""revid":301"#),
            RENDER.replace(r#""pageid":42"#, r#""pageid":43"#),
            RENDER.replace("Énergie", "Another"),
            RENDER.replace(r#","revid":300"#, ""),
        ] {
            assert_eq!(
                parse_rendered_revision(&bad, &revision),
                Err(OnlineArticleError::RevisionMismatch)
            );
        }
    }

    #[test]
    fn query_level_redirects_require_explicit_resolution() {
        let response = META.replacen(
            r#"{"query":{"pages":"#,
            r#"{"query":{"redirects":[{"from":"Alias","to":"Énergie"}],"pages":"#,
            1,
        );
        assert_eq!(
            parse_current_revision(&response, "frwiki", 42),
            Err(OnlineArticleError::RedirectNeedsResolution)
        );
    }

    #[test]
    fn rejects_missing_redirect_multiple_revisions_and_noncanonical_metadata() {
        for (body, expected) in [
            (
                META.replace(r#""pageid":42"#, r#""pageid":0"#),
                OnlineArticleError::InvalidIdentity,
            ),
            (
                META.replace(r#""ns":0"#, r#""ns":1"#),
                OnlineArticleError::InvalidIdentity,
            ),
            (
                META.replace(r#""ns":0"#, r#""ns":2147483648"#),
                OnlineArticleError::InvalidIdentity,
            ),
            (
                META.replace(r#""revid":300"#, r#""revid":0"#),
                OnlineArticleError::InvalidIdentity,
            ),
            (
                META.replace("2026-10-13T10:45:00Z", "not-a-date"),
                OnlineArticleError::InvalidIdentity,
            ),
            (
                META.replace(r#""title":"Énergie""#, r#""title":"Bad\nTitle""#),
                OnlineArticleError::InvalidIdentity,
            ),
            (
                META.replace(
                    r#""revisions":[{"revid":300,"timestamp":"2026-10-13T10:45:00Z"}]"#,
                    r#""revisions":[]"#,
                ),
                OnlineArticleError::InvalidResponse,
            ),
            (
                META.replace(r#""pageid":42"#, r#""missing":true,"pageid":42"#),
                OnlineArticleError::MissingPage,
            ),
            (
                META.replace(r#""pageid":42"#, r#""redirect":true,"pageid":42"#),
                OnlineArticleError::RedirectNeedsResolution,
            ),
        ] {
            assert_eq!(parse_current_revision(&body, "frwiki", 42), Err(expected));
        }
        assert_eq!(
            parse_current_revision(r#"{"error":{"code":"readonly"}}"#, "frwiki", 42),
            Err(OnlineArticleError::ApiError)
        );
        assert_eq!(
            parse_current_revision(&" ".repeat(MAX_METADATA_BYTES + 1), "frwiki", 42),
            Err(OnlineArticleError::ResponseTooLarge)
        );
        assert_eq!(
            parse_current_revision(r#"{"query":{"pages":[]}}"#, "frwiki", 42),
            Err(OnlineArticleError::InvalidResponse)
        );
    }

    #[test]
    fn malformed_rendering_and_unpinned_output_fail_closed() {
        let revision = parse_current_revision(META, "frwiki", 42).unwrap();
        let mut forged_namespace = revision.clone();
        forged_namespace.namespace = 1;
        assert_eq!(
            parse_rendered_revision(RENDER, &forged_namespace),
            Err(OnlineArticleError::InvalidIdentity)
        );
        assert_eq!(
            parse_rendered_revision(
                r#"{"parse":{"pageid":42,"revid":300,"title":"Énergie","text":"<script>bad()</script>"}}"#,
                &revision
            ),
            Err(OnlineArticleError::UnsafeHtml)
        );
        assert_eq!(
            parse_rendered_revision(r#"{"error":{"code":"permissiondenied"}}"#, &revision),
            Err(OnlineArticleError::ApiError)
        );
        assert_eq!(
            parse_rendered_revision(
                r#"{"parse":{"pageid":42,"title":"Énergie","text":"<p>hello</p>"}}"#,
                &revision
            ),
            Err(OnlineArticleError::RevisionMismatch)
        );
    }
}
