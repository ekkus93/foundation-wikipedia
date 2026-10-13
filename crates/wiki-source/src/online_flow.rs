//! Typed navigation flow for revision-safe online article opening.
//!
//! URLs and parsed responses are separate from HTTP transport. Callers must
//! enforce TLS, request deadlines, response-size ceilings, redirect rules and
//! explicit network consent. Rendering is not complete offline-pack evidence.

use wiki_model::ArticleKey;

use crate::online_article::{
    parse_current_revision, parse_rendered_revision, OnlineArticleError, OnlineRenderedRevision,
    OnlineRevision,
};
use crate::online_requests::{OnlineRequestError, WikipediaRequests};
use crate::online_selection::{
    choose_article, ArticleVersion, NetworkState, ReaderSelection, SelectionError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenArticle {
    /// Already have a verified local version, with its original revision label.
    Local(ArticleVersion),
    /// Caller explicitly supplied a current online record; never relabel cached
    /// content as this kind of article.
    CurrentOnline(ArticleVersion),
    /// Only an explicit online request is permitted to use this URL.
    FetchCurrent {
        metadata_url: String,
        fallback: Option<ArticleVersion>,
    },
    MissingOffline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRequest {
    pub pinned: OnlineRevision,
    pub rendered_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineFlowError {
    Selection(SelectionError),
    Request(OnlineRequestError),
    Article(OnlineArticleError),
}

/// Never create an online URL if the network is disabled. For an online fetch,
/// record a separate local fallback instead of passing stale content as current.
pub fn begin_open(
    requested: &ArticleKey,
    network: NetworkState,
    versions: &[ArticleVersion],
) -> Result<OpenArticle, OnlineFlowError> {
    let selection =
        choose_article(requested, network, versions).map_err(OnlineFlowError::Selection)?;
    match selection {
        ReaderSelection::Local(version) => Ok(OpenArticle::Local(version)),
        ReaderSelection::CurrentOnline(version) => Ok(OpenArticle::CurrentOnline(version)),
        ReaderSelection::MissingOffline => Ok(OpenArticle::MissingOffline),
        ReaderSelection::FetchCurrent { fallback } => {
            let api =
                WikipediaRequests::new(&requested.project).map_err(OnlineFlowError::Request)?;
            let metadata_url = api
                .current_revision(requested.page_id)
                .map_err(OnlineFlowError::Request)?;
            Ok(OpenArticle::FetchCurrent {
                metadata_url,
                fallback,
            })
        }
    }
}

/// Decode a fresh metadata response and request HTML ONLY for its exact oldid.
/// Page-ID substitution, redirects and missing/stale revision metadata fail
/// closed before a rendered URL can be produced.
pub fn plan_render(
    requested: &ArticleKey,
    metadata_json: &str,
) -> Result<RenderRequest, OnlineFlowError> {
    let api = WikipediaRequests::new(&requested.project).map_err(OnlineFlowError::Request)?;
    let pinned = parse_current_revision(
        metadata_json,
        &requested.project,
        requested.page_id,
    )
    .map_err(OnlineFlowError::Article)?;
    let rendered_url = api
        .rendered_revision(pinned.revision_id)
        .map_err(OnlineFlowError::Request)?;
    Ok(RenderRequest {
        pinned,
        rendered_url,
    })
}

/// Refuse rendered HTML for any page/revision/title other than the pinned
/// metadata record. The returned HTML has been conservatively sanitized;
/// independent mandatory-media and HTML fidelity checks remain required.
pub fn finish_render(
    plan: &RenderRequest,
    rendered_json: &str,
) -> Result<OnlineRenderedRevision, OnlineFlowError> {
    parse_rendered_revision(rendered_json, &plan.pinned).map_err(OnlineFlowError::Article)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online_selection::ArticleLocation;
    use url::Url;

    fn key() -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id: 42,
        }
    }

    const META: &str = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Physics","revisions":[{"revid":333,"timestamp":"2026-10-13T10:45:00Z"}]}]}}"#;
    const HTML: &str = r#"{"parse":{"pageid":42,"revid":333,"title":"Physics","text":"<p>Physics</p>"}}"#;

    #[test]
    fn metadata_request_to_exact_rendered_revision_roundtrip() {
        let start = begin_open(&key(), NetworkState::Online, &[]).unwrap();
        let OpenArticle::FetchCurrent {
            metadata_url,
            fallback,
        } = start
        else {
            panic!("online current metadata must require a fetch");
        };
        assert_eq!(fallback, None);
        let url = Url::parse(&metadata_url).unwrap();
        assert_eq!(url.host_str(), Some("en.wikipedia.org"));
        assert!(url.query_pairs().any(|(k, v)| k == "pageids" && v == "42"));
        let render = plan_render(&key(), META).unwrap();
        let parsed_url = Url::parse(&render.rendered_url).unwrap();
        assert!(parsed_url.query_pairs().any(|(k, v)| k == "oldid" && v == "333"));
        let article = finish_render(&render, HTML).unwrap();
        assert_eq!(article.revision.revision_id, 333);
        assert!(article.html.contains("Physics"));
        assert_eq!(
            finish_render(&render, &HTML.replace(r#""revid":333"#, r#""revid":334"#)),
            Err(OnlineFlowError::Article(OnlineArticleError::RevisionMismatch))
        );
    }

    #[test]
    fn offline_gates_requests_and_exposes_only_complete_local_versions() {
        let incomplete = ArticleVersion {
            key: key(),
            revision_id: 200,
            location: ArticleLocation::TemporaryCache,
            local_complete: false,
        };
        assert_eq!(
            begin_open(&key(), NetworkState::Offline, &[incomplete.clone()]),
            Ok(OpenArticle::MissingOffline)
        );
        let saved = ArticleVersion {
            key: key(),
            revision_id: 201,
            location: ArticleLocation::ExplicitOfflineSave,
            local_complete: true,
        };
        assert_eq!(
            begin_open(&key(), NetworkState::Offline, &[incomplete.clone(), saved.clone()]),
            Ok(OpenArticle::Local(saved.clone()))
        );
        let start = begin_open(&key(), NetworkState::Online, &[incomplete, saved.clone()]).unwrap();
        let OpenArticle::FetchCurrent { fallback, .. } = start else {
            panic!("online metadata must not be guessed from a saved revision");
        };
        assert_eq!(fallback, Some(saved));
    }

    #[test]
    fn no_render_request_can_follow_a_foreign_or_missing_revision() {
        let foreign = META.replace(r#""pageid":42"#, r#""pageid":43"#);
        assert_eq!(
            plan_render(&key(), &foreign),
            Err(OnlineFlowError::Article(OnlineArticleError::InvalidIdentity))
        );
        assert_eq!(
            plan_render(&key(), r#"{"query":{"pages":[]}}"#),
            Err(OnlineFlowError::Article(OnlineArticleError::InvalidResponse))
        );
        assert_eq!(
            plan_render(&key(), r#"{"error":{"code":"ratelimited"}}"#),
            Err(OnlineFlowError::Article(OnlineArticleError::ApiError))
        );
    }
}
