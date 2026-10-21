//! Transport-agnostic article retrieval, with strict offline and revision gates.
//!
//! A real implementation of MediaWikiTransport MUST enforce verified HTTPS,
//! no off-origin redirects, bounded deadlines, and the requested byte cap.
//! This module itself performs no socket operations, and a parsed page is NOT
//! evidence that required visual assets are locally complete.

use wiki_model::ArticleKey;

use crate::online_article::OnlineRenderedRevision;
use crate::online_flow::{
    begin_open, begin_title_open, finish_render, finish_title_resolution, plan_render,
    OnlineFlowError, OpenArticle, OpenTitle,
};
use crate::online_selection::{ArticleVersion, NetworkState};

const METADATA_BYTES: usize = 1024 * 1024;
const HTML_BYTES: usize = 8 * 1024 * 1024;

/// Strictly limited request adapter. A network implementation must separately
/// enforce trust of the official Wikipedia host, timeouts, and redirect policy.
pub trait MediaWikiTransport {
    type Error;
    fn get(&mut self, url: &str, max_bytes: usize) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchStage {
    Selection,
    TitleResolution,
    Metadata,
    RenderedHtml,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchCause<E> {
    Transport(E),
    ResponseTooLarge,
    InvalidUtf8,
    Flow(OnlineFlowError),
}

/// Preserve the local fallback as an explicitly noncurrent revision on every
/// error; a failed upstream request never silently converts it to current.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchFailure<E> {
    pub stage: FetchStage,
    pub fallback: Option<ArticleVersion>,
    pub cause: FetchCause<E>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchOutcome {
    Local(ArticleVersion),
    CurrentOnline(ArticleVersion),
    MissingOffline,
    Rendered(OnlineRenderedRevision),
}

fn read_response<T: MediaWikiTransport>(
    client: &mut T,
    url: &str,
    max_bytes: usize,
    stage: FetchStage,
    fallback: &Option<ArticleVersion>,
) -> Result<String, FetchFailure<T::Error>> {
    let bytes = client.get(url, max_bytes).map_err(|error| FetchFailure {
        stage,
        fallback: fallback.clone(),
        cause: FetchCause::Transport(error),
    })?;
    if bytes.len() > max_bytes {
        return Err(FetchFailure {
            stage,
            fallback: fallback.clone(),
            cause: FetchCause::ResponseTooLarge,
        });
    }
    String::from_utf8(bytes).map_err(|_| FetchFailure {
        stage,
        fallback: fallback.clone(),
        cause: FetchCause::InvalidUtf8,
    })
}

/// Fetch only the current revision selected through the offline-aware policy.
/// Never fetch any URL for offline state or when a complete local/current record
/// is already selected. Every remote response still requires exact identity
/// and revision matching before returning sanitized HTML.
pub fn fetch_article<T: MediaWikiTransport>(
    client: &mut T,
    requested: &ArticleKey,
    network: NetworkState,
    versions: &[ArticleVersion],
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    let route = begin_open(requested, network, versions).map_err(|cause| FetchFailure {
        stage: FetchStage::Selection,
        fallback: None,
        cause: FetchCause::Flow(cause),
    })?;
    let (url, fallback) = match route {
        OpenArticle::Local(local) => return Ok(FetchOutcome::Local(local)),
        OpenArticle::CurrentOnline(current) => return Ok(FetchOutcome::CurrentOnline(current)),
        OpenArticle::MissingOffline => return Ok(FetchOutcome::MissingOffline),
        OpenArticle::FetchCurrent {
            metadata_url,
            fallback,
        } => (metadata_url, fallback),
    };
    let metadata = read_response(
        client,
        &url,
        METADATA_BYTES,
        FetchStage::Metadata,
        &fallback,
    )?;
    let render = plan_render(requested, &metadata).map_err(|cause| FetchFailure {
        stage: FetchStage::Metadata,
        fallback: fallback.clone(),
        cause: FetchCause::Flow(cause),
    })?;
    let html = read_response(
        client,
        &render.rendered_url,
        HTML_BYTES,
        FetchStage::RenderedHtml,
        &fallback,
    )?;
    let article = finish_render(&render, &html).map_err(|cause| FetchFailure {
        stage: FetchStage::RenderedHtml,
        fallback,
        cause: FetchCause::Flow(cause),
    })?;
    Ok(FetchOutcome::Rendered(article))
}

/// Open a stable page identity obtained from search rather than resolving its
/// mutable display title a second time. Without caller-supplied local versions,
/// an offline lookup cannot return an article.
pub fn fetch_page_id<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    page_id: u64,
    network: NetworkState,
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    fetch_page_id_with_versions(client, project, page_id, network, &[])
}

/// Navigate from a stable page ID while honoring independently verified local
/// versions. A complete installed/saved/cached article is usable offline;
/// online failures preserve that older version as an explicitly local fallback.
/// The caller must verify required media before setting local_complete.
pub fn fetch_page_id_with_versions<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    page_id: u64,
    network: NetworkState,
    versions: &[ArticleVersion],
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    let key = ArticleKey {
        project: project.to_owned(),
        page_id,
    };
    fetch_article(client, &key, network, versions)
}

/// Resolve an explicitly online title through canonical redirect metadata,
/// then read the current page revision and its pinned HTML. Offline title
/// navigation requires the separate local index and must never invoke HTTP.
pub fn fetch_title<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    requested_title: &str,
    network: NetworkState,
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    fetch_title_with_versions(client, project, requested_title, network, &[])
}

/// Resolve an online title to its stable page ID before considering independently
/// verified local versions. A title cannot identify offline content without a
/// trusted local title index; this function makes no network request offline.
/// Local fallbacks are checked against the resolved project/page identity by
/// fetch_article, so a same-title but foreign cached article cannot be used.
pub fn fetch_title_with_versions<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    requested_title: &str,
    network: NetworkState,
    versions: &[ArticleVersion],
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    let route =
        begin_title_open(project, requested_title, network).map_err(|cause| FetchFailure {
            stage: FetchStage::Selection,
            fallback: None,
            cause: FetchCause::Flow(cause),
        })?;
    let title_url = match route {
        OpenTitle::MissingOffline => return Ok(FetchOutcome::MissingOffline),
        OpenTitle::Resolve { title_url } => title_url,
    };

    let response = read_response(
        client,
        &title_url,
        METADATA_BYTES,
        FetchStage::TitleResolution,
        &None,
    )?;
    let resolved =
        finish_title_resolution(project, requested_title, &response).map_err(|cause| {
            FetchFailure {
                stage: FetchStage::TitleResolution,
                fallback: None,
                cause: FetchCause::Flow(cause),
            }
        })?;

    fetch_article(client, &resolved.key, NetworkState::Online, versions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online_selection::ArticleLocation;
    use std::collections::VecDeque;
    use url::Url;

    const METADATA: &str = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Physics","revisions":[{"revid":333,"timestamp":"2026-10-13T10:45:00Z"}]}]}}"#;
    const RENDERED: &str =
        r#"{"parse":{"pageid":42,"revid":333,"title":"Physics","text":"<p>Physics</p>"}}"#;

    #[derive(Default)]
    struct FakeTransport {
        answers: VecDeque<Result<Vec<u8>, &'static str>>,
        requests: Vec<String>,
    }

    impl MediaWikiTransport for FakeTransport {
        type Error = &'static str;

        fn get(&mut self, url: &str, _max_bytes: usize) -> Result<Vec<u8>, Self::Error> {
            self.requests.push(url.to_owned());
            self.answers
                .pop_front()
                .unwrap_or(Err("unexpected HTTP request"))
        }
    }

    fn key() -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id: 42,
        }
    }

    fn local() -> ArticleVersion {
        ArticleVersion {
            key: key(),
            revision_id: 200,
            location: ArticleLocation::ExplicitOfflineSave,
            local_complete: true,
        }
    }

    #[test]
    fn offline_never_contacts_transport_and_returns_local_only() {
        let mut transport = FakeTransport::default();
        assert_eq!(
            fetch_article(&mut transport, &key(), NetworkState::Offline, &[]),
            Ok(FetchOutcome::MissingOffline)
        );
        assert_eq!(
            fetch_article(&mut transport, &key(), NetworkState::Offline, &[local()]),
            Ok(FetchOutcome::Local(local()))
        );
        assert!(transport.requests.is_empty());
    }

    #[test]
    fn metadata_then_exact_oldid_and_sanitized_response() {
        let mut transport = FakeTransport::default();
        transport
            .answers
            .push_back(Ok(METADATA.as_bytes().to_vec()));
        transport
            .answers
            .push_back(Ok(RENDERED.as_bytes().to_vec()));
        let outcome = fetch_article(&mut transport, &key(), NetworkState::Online, &[]).unwrap();
        let FetchOutcome::Rendered(result) = outcome else {
            panic!("expected rendered article");
        };
        assert_eq!(result.revision.revision_id, 333);
        assert_eq!(result.html, "<p>Physics</p>");
        assert_eq!(transport.requests.len(), 2);
        let first = Url::parse(&transport.requests[0]).unwrap();
        let second = Url::parse(&transport.requests[1]).unwrap();
        assert_eq!(first.host_str(), Some("en.wikipedia.org"));
        assert_eq!(second.host_str(), Some("en.wikipedia.org"));
        assert!(first
            .query_pairs()
            .any(|(k, v)| k == "pageids" && v == "42"));
        assert!(second
            .query_pairs()
            .any(|(k, v)| k == "oldid" && v == "333"));
    }

    #[test]
    fn bad_metadata_stops_before_html_request_and_preserves_fallback() {
        let mut transport = FakeTransport::default();
        transport.answers.push_back(Ok(METADATA
            .replace(r#""pageid":42"#, r#""pageid":43"#)
            .into_bytes()));
        let error =
            fetch_article(&mut transport, &key(), NetworkState::Online, &[local()]).unwrap_err();
        assert_eq!(error.stage, FetchStage::Metadata);
        assert_eq!(error.fallback, Some(local()));
        assert!(matches!(error.cause, FetchCause::Flow(_)));
        assert_eq!(transport.requests.len(), 1);
    }

    #[test]
    fn rendered_namespace_mismatch_preserves_local_fallback() {
        let mut transport = FakeTransport::default();
        transport
            .answers
            .push_back(Ok(METADATA.as_bytes().to_vec()));
        let conflicting = RENDERED.replace(r#""title":"Physics""#, r#""ns":1,"title":"Physics""#);
        transport.answers.push_back(Ok(conflicting.into_bytes()));
        let error =
            fetch_article(&mut transport, &key(), NetworkState::Online, &[local()]).unwrap_err();
        assert_eq!(error.stage, FetchStage::RenderedHtml);
        assert_eq!(error.fallback, Some(local()));
        assert_eq!(
            error.cause,
            FetchCause::Flow(OnlineFlowError::Article(
                crate::online_article::OnlineArticleError::RevisionMismatch
            ))
        );
        assert_eq!(transport.requests.len(), 2);
    }

    #[test]
    fn unexpected_query_redirect_stops_before_html_and_preserves_fallback() {
        let mut transport = FakeTransport::default();
        let redirected = r#"{"query":{"redirects":[{"from":"Physics intro","to":"Physics"}],"pages":[{"pageid":42,"ns":0,"title":"Physics","revisions":[{"revid":333,"timestamp":"2026-10-13T10:45:00Z"}]}]}}"#;
        transport
            .answers
            .push_back(Ok(redirected.as_bytes().to_vec()));
        let error =
            fetch_article(&mut transport, &key(), NetworkState::Online, &[local()]).unwrap_err();
        assert_eq!(error.stage, FetchStage::Metadata);
        assert_eq!(error.fallback, Some(local()));
        assert_eq!(
            error.cause,
            FetchCause::Flow(OnlineFlowError::Article(
                crate::online_article::OnlineArticleError::RedirectNeedsResolution
            ))
        );
        assert_eq!(transport.requests.len(), 1);
    }

    #[test]
    fn page_id_from_search_uses_stable_identity_and_never_resolves_title() {
        let mut client = FakeTransport::default();
        client.answers.push_back(Ok(METADATA.as_bytes().to_vec()));
        client.answers.push_back(Ok(RENDERED.as_bytes().to_vec()));
        let result = fetch_page_id(&mut client, "enwiki", 42, NetworkState::Online).unwrap();
        let FetchOutcome::Rendered(article) = result else {
            panic!("expected revision");
        };
        assert_eq!(article.revision.key.page_id, 42);
        assert_eq!(article.revision.revision_id, 333);
        assert_eq!(client.requests.len(), 2);
        assert!(Url::parse(&client.requests[0])
            .unwrap()
            .query_pairs()
            .any(|(key, value)| key == "pageids" && value == "42"));
        assert!(!client.requests[0].contains("titles="));
        assert_eq!(
            fetch_page_id(&mut client, "enwiki", 42, NetworkState::Offline),
            Ok(FetchOutcome::MissingOffline)
        );
        assert_eq!(client.requests.len(), 2);
    }

    #[test]
    fn page_id_navigation_honors_verified_offline_copies_and_online_fallback() {
        let mut client = FakeTransport::default();
        let incomplete_cache = ArticleVersion {
            key: key(),
            revision_id: 201,
            location: ArticleLocation::TemporaryCache,
            local_complete: false,
        };
        assert_eq!(
            fetch_page_id_with_versions(
                &mut client,
                "enwiki",
                42,
                NetworkState::Offline,
                &[incomplete_cache.clone()],

            ),
            Ok(FetchOutcome::MissingOffline)
        );
        assert_eq!(
            fetch_page_id_with_versions(
                &mut client,
                "enwiki",
                42,
                NetworkState::Offline,
                &[incomplete_cache, local()],
            ),
            Ok(FetchOutcome::Local(local()))
        );
        assert!(client.requests.is_empty());

        client.answers.push_back(Err("upstream unavailable"));

        let error = fetch_page_id_with_versions(
            &mut client,
            "enwiki",
            42,
            NetworkState::Online,
            &[local()],
        )
        .unwrap_err();
        assert_eq!(error.stage, FetchStage::Metadata);
        assert_eq!(error.fallback, Some(local()));
        assert_eq!(error.cause, FetchCause::Transport("upstream unavailable"));
        assert_eq!(client.requests.len(), 1);

        let foreign = ArticleVersion {
            key: ArticleKey {
                project: "dewiki".into(),
                page_id: 42,
            },
            ..local()
        };
        let error = fetch_page_id_with_versions(
            &mut client,
            "enwiki",
            42,
            NetworkState::Offline,
            &[foreign],
        )
        .unwrap_err();
        assert_eq!(error.stage, FetchStage::Selection);
        assert_eq!(
            error.cause,
            FetchCause::Flow(OnlineFlowError::Selection(
                crate::online_selection::SelectionError::ForeignArticle
            ))
        );
        assert_eq!(client.requests.len(), 1);
    }

    #[test]
    fn title_fetch_resolves_redirect_then_fetches_revision_and_html() {
        let mut client = FakeTransport::default();
        let title_json = r#"{"query":{"redirects":[{"from":"Physics intro","to":"Physics"}],"pages":[{"pageid":42,"ns":0,"title":"Physics"}]}}"#;
        client.answers.push_back(Ok(title_json.as_bytes().to_vec()));
        client.answers.push_back(Ok(METADATA.as_bytes().to_vec()));
        client.answers.push_back(Ok(RENDERED.as_bytes().to_vec()));
        let result =
            fetch_title(&mut client, "enwiki", "Physics intro", NetworkState::Online).unwrap();
        let FetchOutcome::Rendered(article) = result else {
            panic!("resolved article must contain exact pinned HTML");
        };
        assert_eq!(article.revision.key, key());
        assert_eq!(client.requests.len(), 3);
        for (index, expected) in [
            ("titles", "Physics intro"),
            ("pageids", "42"),
            ("oldid", "333"),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(Url::parse(&client.requests[index])
                .unwrap()
                .query_pairs()
                .any(|(key, val)| key == expected.0 && val == expected.1));
        }
    }

    #[test]
    fn title_navigation_preserves_verified_local_fallback_after_upstream_failure() {
        let mut client = FakeTransport::default();
        let title_json = r#"{"query":{"redirects":[{"from":"Physics intro","to":"Physics"}],"pages":[{"pageid":42,"ns":0,"title":"Physics"}]}}"#;
        client.answers.push_back(Ok(title_json.as_bytes().to_vec()));
        client
            .answers
            .push_back(Err("revision service unavailable"));
        let error = fetch_title_with_versions(
            &mut client,
            "enwiki",
            "Physics intro",
            NetworkState::Online,
            &[local()],
        )
        .unwrap_err();
        assert_eq!(error.stage, FetchStage::Metadata);
        assert_eq!(
            error.cause,
            FetchCause::Transport("revision service unavailable")
        );
        assert_eq!(error.fallback, Some(local()));
        assert_eq!(client.requests.len(), 2);
    }

    #[test]
    fn title_navigation_rejects_foreign_fallback_and_never_guesses_offline_title() {
        let mut client = FakeTransport::default();
        assert_eq!(
            fetch_title_with_versions(
                &mut client,
                "enwiki",
                "Physics",
                NetworkState::Offline,
                &[local()],
            ),
            Ok(FetchOutcome::MissingOffline)
        );
        assert!(client.requests.is_empty());

        let title_json = r#"{"query":{"pages":[{"pageid":43,"ns":0,"title":"Physics"}]}}"#;
        client.answers.push_back(Ok(title_json.as_bytes().to_vec()));
        let error = fetch_title_with_versions(
            &mut client,
            "enwiki",
            "Physics",
            NetworkState::Online,
            &[local()],
        )
        .unwrap_err();
        assert_eq!(error.stage, FetchStage::Selection);
        assert_eq!(error.fallback, None);
        assert_eq!(
            error.cause,
            FetchCause::Flow(OnlineFlowError::Selection(
                crate::online_selection::SelectionError::ForeignArticle
            ))
        );
        assert_eq!(client.requests.len(), 1);
    }

    #[test]
    fn title_fetch_never_uses_network_offline_or_after_failed_resolution() {
        let mut client = FakeTransport::default();
        assert_eq!(
            fetch_title(&mut client, "enwiki", "Physics", NetworkState::Offline),
            Ok(FetchOutcome::MissingOffline)
        );
        assert!(client.requests.is_empty());
        let forged = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Other"}]}}"#;
        client.answers.push_back(Ok(forged.as_bytes().to_vec()));
        let error =
            fetch_title(&mut client, "enwiki", "Physics", NetworkState::Online).unwrap_err();
        assert_eq!(error.stage, FetchStage::TitleResolution);
        assert!(matches!(error.cause, FetchCause::Flow(_)));
        assert_eq!(client.requests.len(), 1);
    }

    #[test]
    fn network_error_or_oversized_bytes_are_not_displayed_as_current() {
        let mut unavailable = FakeTransport::default();
        unavailable.answers.push_back(Err("network offline"));
        let error =
            fetch_article(&mut unavailable, &key(), NetworkState::Online, &[local()]).unwrap_err();
        assert_eq!(error.cause, FetchCause::Transport("network offline"));
        assert_eq!(error.fallback, Some(local()));
        let mut oversized = FakeTransport::default();
        oversized
            .answers
            .push_back(Ok(vec![b' '; METADATA_BYTES + 1]));
        let error = fetch_article(&mut oversized, &key(), NetworkState::Online, &[]).unwrap_err();
        assert_eq!(error.stage, FetchStage::Metadata);
        assert_eq!(error.cause, FetchCause::ResponseTooLarge);
        assert_eq!(oversized.requests.len(), 1);
    }
}
