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
use crate::online_title::{
    parse_title_resolution, ResolvedTitle, TitleResolutionError,
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
pub enum OpenTitle {
    /// A title alone is not sufficient to identify a persisted offline article.
    /// Caller may search a trusted local title index without any network access.
    MissingOffline,
    /// On an explicitly online path, resolve title/redirects before fetching
    /// the current page ID; the URL is not an authenticated network response.
    Resolve { title_url: String },
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
    Title(TitleResolutionError),
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

/// Request authoritative title resolution on an explicitly online path only.
/// This never treats an input title as a stable page/revision identifier.
pub fn begin_title_open(
    project: &str,
    title: &str,
    network: NetworkState,
) -> Result<OpenTitle, OnlineFlowError> {
    let api = WikipediaRequests::new(project).map_err(OnlineFlowError::Request)?;
    // Validate even if offline so bad titles are not passed on to local callers.
    let title_url = api.resolve_title(title).map_err(OnlineFlowError::Request)?;
    match network {
        NetworkState::Online => Ok(OpenTitle::Resolve { title_url }),
        NetworkState::Offline => Ok(OpenTitle::MissingOffline),
    }
}

/// Interpret a trusted-transport title-resolution response as a stable page
/// identity. Caller can then call begin_open on the resolved page ID.
pub fn finish_title_resolution(
    project: &str,
    requested_title: &str,
    response_json: &str,
) -> Result<ResolvedTitle, OnlineFlowError> {
    parse_title_resolution(response_json, project, requested_title)
        .map_err(OnlineFlowError::Title)
}

/// Decode a fresh metadata response and request HTML ONLY for its exact oldid.
/// Page-ID substitution, redirects and missing/stale revision metadata fail
/// closed before a rendered URL can be produced.
pub fn plan_render(
    requested: &ArticleKey,
    metadata_json: &str,
) -> Result<RenderRequest, OnlineFlowError> {
    let api = WikipediaRequests::new(&requested.project).map_err(OnlineFlowError::Request)?;
    let pinned = parse_current_revision(metadata_json, &requested.project, requested.page_id)
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
    const HTML: &str =
        r#"{"parse":{"pageid":42,"revid":333,"title":"Physics","text":"<p>Physics</p>"}}"#;

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
        assert!(parsed_url
            .query_pairs()
            .any(|(k, v)| k == "oldid" && v == "333"));
        let article = finish_render(&render, HTML).unwrap();
        assert_eq!(article.revision.revision_id, 333);
        assert!(article.html.contains("Physics"));
        assert_eq!(
            finish_render(&render, &HTML.replace(r#""revid":333"#, r#""revid":334"#)),
            Err(OnlineFlowError::Article(
                OnlineArticleError::RevisionMismatch
            ))
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
            begin_open(
                &key(),
                NetworkState::Offline,
                &[incomplete.clone(), saved.clone()]
            ),
            Ok(OpenArticle::Local(saved.clone()))
        );
        let start = begin_open(&key(), NetworkState::Online, &[incomplete, saved.clone()]).unwrap();
        let OpenArticle::FetchCurrent { fallback, .. } = start else {
            panic!("online metadata must not be guessed from a saved revision");
        };
        assert_eq!(fallback, Some(saved));
    }

    #[test]
    fn title_navigation_resolves_redirect_to_stable_page_before_fetch() {
        let begin = begin_title_open("enwiki", "physics_intro", NetworkState::Online).unwrap();
        let OpenTitle::Resolve { title_url } = begin else {
            panic!("online title lookup must resolve canonical page ID first");
        };
        let request = Url::parse(&title_url).unwrap();
        assert_eq!(request.host_str(), Some("en.wikipedia.org"));
        assert!(request
            .query_pairs()
            .any(|(key, value)| key == "titles" && value == "physics_intro"));
        let response = r#"{"query":{"normalized":[{"from":"physics_intro","to":"Physics intro"}],"redirects":[{"from":"Physics intro","to":"Physics"}],"pages":[{"pageid":42,"ns":0,"title":"Physics"}]}}"#;
        let resolved = finish_title_resolution("enwiki", "physics_intro", response).unwrap();
        assert_eq!(resolved.key, key());
        assert_eq!(resolved.canonical_title, "Physics");
        assert_eq!(resolved.edges.len(), 2);
        let page = begin_open(&resolved.key, NetworkState::Online, &[]).unwrap();
        let OpenArticle::FetchCurrent { metadata_url, .. } = page else {
            panic!("resolved title must still require current revision fetch");
        };
        assert!(Url::parse(&metadata_url)
            .unwrap()
            .query_pairs()
            .any(|(key, value)| key == "pageids" && value == "42"));
    }

    #[test]
    fn offline_title_navigation_never_attempts_url_fetch_and_rejects_bad_titles() {
        assert_eq!(
            begin_title_open("enwiki", "Physics", NetworkState::Offline),
            Ok(OpenTitle::MissingOffline)
        );
        assert_eq!(
            begin_title_open("enwiki", " A", NetworkState::Offline),
            Err(OnlineFlowError::Request(OnlineRequestError::InvalidTitle))
        );
        assert_eq!(
            begin_title_open("xx.invalid", "Physics", NetworkState::Offline),
            Err(OnlineFlowError::Request(OnlineRequestError::InvalidProject))
        );
        let wrong = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Other"}]}}"#;
        assert_eq!(
            finish_title_resolution("enwiki", "Physics", wrong),
            Err(OnlineFlowError::Title(TitleResolutionError::RedirectChain))
        );
        let api_error = r#"{"error":{"code":"invalidtitle"}}"#;
        assert_eq!(
            finish_title_resolution("enwiki", "Physics", api_error),
            Err(OnlineFlowError::Title(TitleResolutionError::ApiError))
        );
    }

    #[test]
    fn no_render_request_can_follow_a_foreign_or_missing_revision() {
        let foreign = META.replace(r#""pageid":42"#, r#""pageid":43"#);
        assert_eq!(
            plan_render(&key(), &foreign),
            Err(OnlineFlowError::Article(
                OnlineArticleError::InvalidIdentity
            ))
        );
        assert_eq!(
            plan_render(&key(), r#"{"query":{"pages":[]}}"#),
            Err(OnlineFlowError::Article(
                OnlineArticleError::InvalidResponse
            ))
        );
        assert_eq!(
            plan_render(&key(), r#"{"error":{"code":"ratelimited"}}"#),
            Err(OnlineFlowError::Article(OnlineArticleError::ApiError))
        );
    }
}
