//! Transport-agnostic article retrieval, with strict offline and revision gates.
//!
//! A real implementation of MediaWikiTransport MUST enforce verified HTTPS,
//! no off-origin redirects, bounded deadlines, and the requested byte cap.
//! This module itself performs no socket operations, and a parsed page is NOT
//! evidence that required visual assets are locally complete.

use wiki_model::ArticleKey;

use crate::online_article::OnlineRenderedRevision;
use crate::online_flow::{begin_open, finish_render, plan_render, OnlineFlowError, OpenArticle};
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
