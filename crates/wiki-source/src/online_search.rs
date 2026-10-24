//! Parse bounded MediaWiki Action API search results into navigation identities.
//!
//! This adapter consumes a caller-supplied response from an explicitly
//! selected Wikimedia endpoint. It does not fetch data, establish source
//! authenticity, render article HTML, or authorize AI evidence citations.

use std::collections::HashSet;

use serde_json::Value;
use wiki_model::{wikipedia_title_key, ArticleKey};

use crate::local_title_index::{LocalSearchHit, LocalTitleIndex, LocalTitleIndexError};
use crate::online_fetch::MediaWikiTransport;
use crate::online_requests::{OnlineRequestError, WikipediaRequests};
use crate::online_selection::NetworkState;

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
    DuplicateTitle,
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
        crate::strict_json::parse(body).map_err(|_| OnlineSearchError::InvalidResponse)?;
    if value.get("error").is_some() {
        return Err(OnlineSearchError::ApiError);
    }
    if value
        .get("batchcomplete")
        .is_some_and(|flag| flag.as_bool() != Some(true))
    {
        return Err(OnlineSearchError::InvalidResponse);
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
    let mut seen_titles = HashSet::new();
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
            .filter(|namespace| *namespace == 0)
            .ok_or(OnlineSearchError::InvalidIdentity)?;
        let title = result
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| crate::online_requests::safe_navigation_text(title, MAX_TITLE_BYTES))
            .ok_or(OnlineSearchError::InvalidIdentity)?;
        if !seen.insert(page_id) {
            return Err(OnlineSearchError::DuplicatePage);
        }
        // Reuse the model's first-letter and underscore normalization:
        // distinct page IDs cannot claim the same navigation title key,
        // including beyond the visible result limit.
        if !seen_titles.insert(wikipedia_title_key(title)) {
            return Err(OnlineSearchError::DuplicateTitle);
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

/// Search metadata is for navigation, never revision-pinned citation evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchOutcome {
    OfflineNeedsLocalIndex,
    Results(Vec<OnlineSearchHit>),
    /// Network-free prefix matches from independently verified local content.
    LocalResults(Vec<LocalSearchHit>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchFetchError<E> {
    Request(OnlineRequestError),
    Transport(E),
    ResponseTooLarge,
    InvalidUtf8,
    Parse(OnlineSearchError),
    LocalIndex(LocalTitleIndexError),
    /// Transport/unreadable-response failure with verified local title
    /// suggestions. The caller may show these as explicitly offline results;
    /// they must not be presented as a successful current Wikimedia search.
    UnavailableWithLocal {
        reason: Box<SearchFetchError<E>>,
        hits: Vec<LocalSearchHit>,
    },
}

/// Explicit online-only search through an injected HTTPS-capable transport.
/// Implementations MUST verify TLS/origin, forbid off-origin redirects and
/// enforce timeouts. A second response-size check protects against a client
/// that ignores its byte limit.
pub fn fetch_search<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    text: &str,
    limit: usize,
    network: NetworkState,
) -> Result<SearchOutcome, SearchFetchError<T::Error>> {
    let api = WikipediaRequests::new(project).map_err(SearchFetchError::Request)?;
    let url = api.search(text, limit).map_err(SearchFetchError::Request)?;
    if network == NetworkState::Offline {
        return Ok(SearchOutcome::OfflineNeedsLocalIndex);
    }
    let bytes = client
        .get(&url, MAX_RESPONSE_BYTES)
        .map_err(SearchFetchError::Transport)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(SearchFetchError::ResponseTooLarge);
    }
    let body = String::from_utf8(bytes).map_err(|_| SearchFetchError::InvalidUtf8)?;
    let hits = parse_search_results(&body, project, limit).map_err(SearchFetchError::Parse)?;
    Ok(SearchOutcome::Results(hits))
}

/// Search verified local titles while offline, without touching the transport.
/// Online behavior retains the existing official API flow. A missing local
/// index remains an explicit state rather than silently contacting the network.
/// A project mismatch is an error, not a cross-language result leak.
pub fn fetch_search_with_local<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    text: &str,
    limit: usize,
    network: NetworkState,
    local: Option<&LocalTitleIndex>,
) -> Result<SearchOutcome, SearchFetchError<T::Error>> {
    let api = WikipediaRequests::new(project).map_err(SearchFetchError::Request)?;
    api.search(text, limit).map_err(SearchFetchError::Request)?;
    if let Some(index) = local {
        if index.project() != project {
            return Err(SearchFetchError::LocalIndex(
                LocalTitleIndexError::ForeignProject,
            ));
        }
    }
    if network == NetworkState::Offline {
        let Some(index) = local else {
            return Ok(SearchOutcome::OfflineNeedsLocalIndex);
        };
        return index
            .search_prefix(text, limit)
            .map(SearchOutcome::LocalResults)
            .map_err(SearchFetchError::LocalIndex);
    }
    match fetch_search(client, project, text, limit, network) {
        Err(
            reason @ (SearchFetchError::Transport(_)
            | SearchFetchError::ResponseTooLarge
            | SearchFetchError::InvalidUtf8),
        ) => {
            // An independently validated local index is usable when upstream
            // transport becomes unavailable. Retain the failure as an error
            // so applications explicitly label these as local candidates.
            let hits = if let Some(index) = local {
                index
                    .search_prefix(text, limit)
                    .map_err(SearchFetchError::LocalIndex)?
            } else {
                Vec::new()
            };
            if hits.is_empty() {
                Err(reason)
            } else {
                Err(SearchFetchError::UnavailableWithLocal {
                    reason: Box::new(reason),
                    hits,
                })
            }
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_search_transport_failure_exposes_only_verified_local_suggestions() {
        use crate::local_title_index::{LocalTitleEntry, LocalTitleRedirect};
        use crate::online_selection::{ArticleLocation, ArticleVersion};
        use std::collections::VecDeque;

        #[derive(Default)]
        struct Fake {
            answers: VecDeque<Result<Vec<u8>, &'static str>>,
            calls: usize,
        }
        impl MediaWikiTransport for Fake {
            type Error = &'static str;
            fn get(&mut self, _url: &str, _limit: usize) -> Result<Vec<u8>, Self::Error> {
                self.calls += 1;
                self.answers.pop_front().unwrap_or(Err("offline"))
            }
        }
        let index = LocalTitleIndex::new(
            "enwiki",
            [LocalTitleEntry {
                title: "Physics".into(),
                version: ArticleVersion {
                    key: ArticleKey {
                        project: "enwiki".into(),
                        page_id: 42,
                    },
                    revision_id: 123,
                    location: ArticleLocation::InstalledPack,
                    local_complete: true,
                },
            }],
        )
        .unwrap()
        .with_redirects([LocalTitleRedirect {
            alias: "Physical science".into(),
            canonical_title: "Physics".into(),
            target_page_id: 42,
        }])
        .unwrap();

        let mut offline = Fake::default();
        assert_eq!(
            fetch_search_with_local(
                &mut offline,
                "enwiki",
                "Phys",
                5,
                NetworkState::Offline,
                Some(&index),
            ),
            Ok(SearchOutcome::LocalResults(
                index.search_prefix("Phys", 5).unwrap()
            ))
        );
        assert_eq!(offline.calls, 0);

        let mut client = Fake::default();
        client.answers.push_back(Err("offline"));
        let failure = fetch_search_with_local(
            &mut client,
            "enwiki",
            "Phys",
            5,
            NetworkState::Online,
            Some(&index),
        )
        .unwrap_err();
        let SearchFetchError::UnavailableWithLocal { reason, hits } = failure else {
            panic!("expected verified local candidates on transport outage");
        };
        assert_eq!(*reason, SearchFetchError::Transport("offline"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].hit.key.page_id, 42);
        assert_eq!(hits[0].version.revision_id, 123);
        assert_eq!(hits[0].version.location, ArticleLocation::InstalledPack);
        assert_eq!(client.calls, 1);

        let mut no_match = Fake::default();
        assert_eq!(
            fetch_search_with_local(
                &mut no_match,
                "enwiki",
                "unknown",
                5,
                NetworkState::Online,
                Some(&index),
            ),
            Err(SearchFetchError::Transport("offline"))
        );
        let mut no_index = Fake::default();
        assert_eq!(
            fetch_search_with_local(
                &mut no_index,
                "enwiki",
                "Phys",
                5,
                NetworkState::Online,
                None,
            ),
            Err(SearchFetchError::Transport("offline"))
        );

        let mut response = Fake::default();
        response.answers.push_back(Ok(
            br#"{"query":{"search":[{"pageid":99,"ns":0,"title":"Physics"}]}}"#.to_vec(),
        ));
        let outcome = fetch_search_with_local(
            &mut response,
            "enwiki",
            "Phys",
            5,
            NetworkState::Online,
            Some(&index),
        )
        .unwrap();
        let SearchOutcome::Results(hits) = outcome else {
            panic!("online result must take precedence over the local suggestion");
        };
        assert_eq!(hits[0].key.page_id, 99);

        let mut malformed = Fake::default();
        malformed.answers.push_back(Ok(b"not JSON".to_vec()));
        assert_eq!(
            fetch_search_with_local(
                &mut malformed,
                "enwiki",
                "Phys",
                5,
                NetworkState::Online,
                Some(&index),
            ),
            Err(SearchFetchError::Parse(OnlineSearchError::InvalidResponse))
        );

        let mut wrong_project = Fake::default();
        assert_eq!(
            fetch_search_with_local(
                &mut wrong_project,
                "frwiki",
                "Phys",
                5,
                NetworkState::Online,
                Some(&index),
            ),
            Err(SearchFetchError::LocalIndex(
                LocalTitleIndexError::ForeignProject
            ))
        );
        assert_eq!(wrong_project.calls, 0);
    }

    #[test]
    fn rejects_duplicate_json_keys_in_online_search() {
        for body in [
            r#"{"query":{"search":[{"pageid":42,"pageid":43,"ns":0,"title":"Physics"}]}}"#,
            r#"{"query":{"search":[{"pageid":42,"ns":0,"title":"Physics","t\u0069tle":"Chemistry"}]}}"#,
            r#"{"query":{"search":[{"pageid":42,"ns":0,"title":"Physics","snippet":{"x":1,"x":2}}]}}"#,
            r#"{"query":{"search":[{"pageid":42,"ns":0,"title":"Physics"}]},"query":{"search":[]}}"#,
        ] {
            assert_eq!(
                parse_search_results(body, "enwiki", 1),
                Err(OnlineSearchError::InvalidResponse),
                "{body}"
            );
        }
        assert_eq!(
            parse_search_results(
                r#"{"query":{"search":[{"pageid":42,"ns":0,"title":"Énergie"}]}}"#,
                "enwiki",
                1
            )
            .unwrap()[0]
                .title,
            "Énergie"
        );
    }

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
        let conflicting_titles = r#"{"query":{"search":[
            {"pageid":1,"ns":0,"title":"Physics"},
            {"pageid":2,"ns":0,"title":"Physics"}]}}"#;
        assert_eq!(
            parse_search_results(conflicting_titles, "enwiki", 1),
            Err(OnlineSearchError::DuplicateTitle)
        );
        assert_eq!(
            parse_search_results(conflicting_titles, "enwiki", 0),
            Err(OnlineSearchError::DuplicateTitle)
        );
        let underscore_aliases = r#"{"query":{"search":[
            {"pageid":1,"ns":0,"title":"A_B"},
            {"pageid":2,"ns":0,"title":"A B"}]}}"#;
        assert_eq!(
            parse_search_results(underscore_aliases, "enwiki", 0),
            Err(OnlineSearchError::DuplicateTitle)
        );
        let first_letter_aliases = r#"{"query":{"search":[
            {"pageid":1,"ns":0,"title":"physics"},
            {"pageid":2,"ns":0,"title":"Physics"}]}}"#;
        assert_eq!(
            parse_search_results(first_letter_aliases, "enwiki", 1),
            Err(OnlineSearchError::DuplicateTitle)
        );
        for bad in [
            r#"{"query":{"search":[{"pageid":0,"ns":0,"title":"A"}]}}"#,
            r#"{"query":{"search":[{"pageid":9223372036854775808,"ns":0,"title":"A"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":2147483648,"title":"A"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":" "} ]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"A\nB"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"A\u061cB"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"A\u200eB"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"A\u200fB"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"A\u202eB"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":1,"title":"Talk:Physics"}]}}"#,
            r#"{"query":{"search":[{"pageid":1,"ns":0,"title":" Physics "}]}}"#,
        ] {
            assert_eq!(
                parse_search_results(bad, "enwiki", 0),
                Err(OnlineSearchError::InvalidIdentity),
                "{bad}"
            );
        }
    }

    #[test]
    fn search_parser_rejects_invisible_and_noncharacter_titles() {
        let valid = r#"{"query":{"search":[{"pageid":1,"ns":0,"title":"Physics"}]}}"#;
        for marker in [
            '\u{200b}',
            '\u{2028}',
            '\u{2060}',
            '\u{206a}',
            '\u{fdd0}',
            '\u{ffff}',
            '\u{10ffff}',
        ] {
            let body = valid.replace("Physics", &format!("Physics{marker}"));
            assert_eq!(
                parse_search_results(&body, "enwiki", 0),
                Err(OnlineSearchError::InvalidIdentity)
            );
        }
    }

    #[test]
    fn rejects_explicitly_incomplete_search_batches() {
        assert_eq!(
            parse_search_results(
                r#"{"batchcomplete":false,"query":{"search":[]}}"#,
                "enwiki",
                5
            ),
            Err(OnlineSearchError::InvalidResponse)
        );
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

    #[test]
    fn online_search_respects_offline_gate_and_exact_response_limits() {
        use std::collections::VecDeque;
        use url::Url;

        #[derive(Default)]
        struct Fake {
            responses: VecDeque<Result<Vec<u8>, &'static str>>,
            requests: Vec<String>,
        }
        impl MediaWikiTransport for Fake {
            type Error = &'static str;
            fn get(&mut self, url: &str, limit: usize) -> Result<Vec<u8>, Self::Error> {
                assert_eq!(limit, MAX_RESPONSE_BYTES);
                self.requests.push(url.to_owned());
                self.responses.pop_front().unwrap_or(Err("no response"))
            }
        }

        let mut client = Fake::default();
        assert_eq!(
            fetch_search(&mut client, "enwiki", "Physics", 1, NetworkState::Offline),
            Ok(SearchOutcome::OfflineNeedsLocalIndex)
        );
        assert!(client.requests.is_empty());
        assert_eq!(
            fetch_search(&mut client, "enwiki", "bad\nquery", 1, NetworkState::Online),
            Err(SearchFetchError::Request(OnlineRequestError::InvalidSearch))
        );
        assert!(client.requests.is_empty());

        let body = br#"{"query":{"search":[{"pageid":42,"ns":0,"title":"Physics","snippet":"<script>bad()</script>"}]}}"#;
        client.responses.push_back(Ok(body.to_vec()));
        let result = fetch_search(
            &mut client,
            "enwiki",
            "Énergie &list=allusers",
            1,
            NetworkState::Online,
        )
        .unwrap();
        let SearchOutcome::Results(hits) = result else {
            panic!("expected online navigation hits");
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].key.page_id, 42);
        let url = Url::parse(&client.requests[0]).unwrap();
        assert_eq!(url.host_str(), Some("en.wikipedia.org"));
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "srsearch" && value == "Énergie &list=allusers"));

        client.responses.push_back(Err("network unavailable"));
        assert_eq!(
            fetch_search(&mut client, "enwiki", "Physics", 1, NetworkState::Online),
            Err(SearchFetchError::Transport("network unavailable"))
        );
