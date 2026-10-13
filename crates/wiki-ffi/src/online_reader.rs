//! Bridge revision-pinned Wikimedia responses to disposable HTML cache.
//! TLS transport, trusted source provenance and required-media completeness
//! remain separate obligations; a temporary cache is never an offline pack.

use wiki_model::ArticleKey;
use wiki_source::online_fetch::{
    fetch_article, fetch_title, FetchFailure, FetchOutcome, MediaWikiTransport,
};
use wiki_source::online_selection::{ArticleLocation, ArticleVersion, NetworkState};
use wiki_store::online_cache::{CachedHtml, HtmlCacheError, RevisionHtmlCache};

#[derive(Debug)]
pub enum CachedFetchError<E> {
    InvalidFetchTime,
    /// Upstream must not silently replace a previously observed newer
    /// revision with an older current-revision response.
    RevisionRegression {
        newest_cached: u64,
        incoming: u64,
    },
    Fetch(FetchFailure<E>),
    Cache(HtmlCacheError),
}

fn persist<E>(
    cache: &mut RevisionHtmlCache,
    result: &FetchOutcome,
    fetched_at: i64,
) -> Result<(), CachedFetchError<E>> {
    if let FetchOutcome::Rendered(article) = result {
        if let Some(newest) = latest_temporary_version(cache, &article.revision.key)
            .map_err(CachedFetchError::Cache)?
        {
            if newest.revision_id > article.revision.revision_id {
                return Err(CachedFetchError::RevisionRegression {
                    newest_cached: newest.revision_id,
                    incoming: article.revision.revision_id,
                });
            }
        }
        cache
            .put(&CachedHtml {
                key: article.revision.key.clone(),
                revision_id: article.revision.revision_id,
                title: article.revision.title.clone(),
                html: article.html.clone(),
                fetched_at,
            })
            .map_err(CachedFetchError::Cache)?;
    }
    Ok(())
}

/// Return the highest verified revision ID, not the most recently fetched
/// temporary HTML entry. A later refresh of an older revision must not make
/// the reader advertise that revision as its newest cached candidate.
/// The stored HTML alone never proves that images, math, and other required
/// visual objects exist locally; only an independent completeness verifier
/// may authorize a true offline/saved article version.
pub fn latest_temporary_version(
    cache: &RevisionHtmlCache,
    key: &ArticleKey,
) -> Result<Option<ArticleVersion>, HtmlCacheError> {
    Ok(cache
        .list_cached_revisions(key)?
        .into_iter()
        .max_by_key(|article| article.revision_id)
        .map(|article| ArticleVersion {
            key: article.key,
            revision_id: article.revision_id,
            location: ArticleLocation::TemporaryCache,
            local_complete: false,
        }))
}

/// Only a successful exact-oldid render may populate temporary storage.
/// Offline and preexisting local-version responses never write to this cache.
pub fn fetch_page_cached<T: MediaWikiTransport>(
    transport: &mut T,
    cache: &mut RevisionHtmlCache,
    key: &ArticleKey,
    network: NetworkState,
    versions: &[ArticleVersion],
    fetched_at: i64,
) -> Result<FetchOutcome, CachedFetchError<T::Error>> {
    if network == NetworkState::Online && fetched_at <= 0 {
        return Err(CachedFetchError::InvalidFetchTime);
    }
    let outcome =
        fetch_article(transport, key, network, versions).map_err(CachedFetchError::Fetch)?;
    persist(cache, &outcome, fetched_at)?;
    Ok(outcome)
}

/// Title aliases are resolved before caching under the actual page identity.
pub fn fetch_title_cached<T: MediaWikiTransport>(
    transport: &mut T,
    cache: &mut RevisionHtmlCache,
    project: &str,
    title: &str,
    network: NetworkState,
    fetched_at: i64,
) -> Result<FetchOutcome, CachedFetchError<T::Error>> {
    if network == NetworkState::Online && fetched_at <= 0 {
        return Err(CachedFetchError::InvalidFetchTime);
    }
    let outcome =
        fetch_title(transport, project, title, network).map_err(CachedFetchError::Fetch)?;
    persist(cache, &outcome, fetched_at)?;
    Ok(outcome)
}

/// Host/desktop-only online search; returns navigation identities only,
/// never revision-pinned article evidence. Offline mode needs the local index.
#[cfg(not(target_os = "android"))]
pub fn fetch_search_host_https(
    project: &str,
    query: &str,
    limit: usize,
    network: NetworkState,
) -> Result<
    wiki_source::online_search::SearchOutcome,
    wiki_source::online_search::SearchFetchError<wiki_source::online_curl::CurlTransportError>,
> {
    let mut transport = wiki_source::online_curl::CurlTransport::new();
    wiki_source::online_search::fetch_search(&mut transport, project, query, limit, network)
}

/// Host/desktop-only concrete HTTPS path. The caller explicitly chooses
/// NetworkState::Online; offline requests never start a curl process.
/// Android must use a native certificate-verifying transport instead.
#[cfg(not(target_os = "android"))]
pub fn fetch_page_cached_host_https(
    cache: &mut RevisionHtmlCache,
    key: &ArticleKey,
    network: NetworkState,
    versions: &[ArticleVersion],
    fetched_at: i64,
) -> Result<FetchOutcome, CachedFetchError<wiki_source::online_curl::CurlTransportError>> {
    let mut transport = wiki_source::online_curl::CurlTransport::new();
    fetch_page_cached(&mut transport, cache, key, network, versions, fetched_at)
}

/// Host/desktop-only title navigation through the same verified-HTTPS adapter.
/// Redirect and revision identities remain independently checked before caching.
#[cfg(not(target_os = "android"))]
pub fn fetch_title_cached_host_https(
    cache: &mut RevisionHtmlCache,
    project: &str,
    title: &str,
    network: NetworkState,
    fetched_at: i64,
) -> Result<FetchOutcome, CachedFetchError<wiki_source::online_curl::CurlTransportError>> {
    let mut transport = wiki_source::online_curl::CurlTransport::new();
    fetch_title_cached(&mut transport, cache, project, title, network, fetched_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "android"))]
    #[test]
    fn host_transport_entrypoints_never_spawn_curl_while_offline() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        assert_eq!(
            fetch_page_cached_host_https(&mut cache, &key(), NetworkState::Offline, &[], 0)
                .unwrap(),
            FetchOutcome::MissingOffline
        );
        assert_eq!(
            fetch_title_cached_host_https(
                &mut cache,
                "enwiki",
                "Physics",
                NetworkState::Offline,
                0,
            )
            .unwrap(),
            FetchOutcome::MissingOffline
        );
        assert_eq!(cache.get(&key(), 333).unwrap(), None);
        assert_eq!(
            fetch_search_host_https("enwiki", "Physics", 3, NetworkState::Offline).unwrap(),
            wiki_source::online_search::SearchOutcome::OfflineNeedsLocalIndex
        );
    }

    use std::collections::VecDeque;

    const META: &str = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Physics","revisions":[{"revid":333,"timestamp":"2026-10-13T10:45:00Z"}]}]}}"#;
    const HTML: &str =
        r#"{"parse":{"pageid":42,"revid":333,"title":"Physics","text":"<p>Physics</p>"}}"#;

    #[derive(Default)]
    struct Fake {
        replies: VecDeque<Vec<u8>>,
        calls: usize,
    }

    impl MediaWikiTransport for Fake {
        type Error = &'static str;
        fn get(&mut self, _url: &str, _limit: usize) -> Result<Vec<u8>, Self::Error> {
            self.calls += 1;
            self.replies.pop_front().ok_or("missing reply")
        }
    }

    fn key() -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id: 42,
        }
    }

    fn good() -> Fake {
        Fake {
            replies: VecDeque::from(vec![META.as_bytes().to_vec(), HTML.as_bytes().to_vec()]),
            calls: 0,
        }
    }

    #[test]
    fn online_revision_is_stored_and_identical_refresh_updates_timestamp() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let mut first = good();
        assert!(matches!(
            fetch_page_cached(
                &mut first,
                &mut cache,
                &key(),
                NetworkState::Online,
                &[],
                100
            ),
            Ok(FetchOutcome::Rendered(_))
        ));
        assert_eq!(first.calls, 2);
        assert_eq!(
            cache.get(&key(), 333).unwrap().unwrap().html,
            "<p>Physics</p>"
        );
        fetch_page_cached(
            &mut good(),
            &mut cache,
            &key(),
            NetworkState::Online,
            &[],
            200,
        )
        .unwrap();
        assert_eq!(cache.get(&key(), 333).unwrap().unwrap().fetched_at, 200);
    }

    #[test]
    fn offline_invalid_time_and_wrong_render_do_not_cache() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let mut offline = good();
        assert_eq!(
            fetch_page_cached(
                &mut offline,
                &mut cache,
                &key(),
                NetworkState::Offline,
                &[],
                0
            )
            .unwrap(),
            FetchOutcome::MissingOffline
        );
        assert!(matches!(
            fetch_page_cached(
                &mut offline,
                &mut cache,
                &key(),
                NetworkState::Online,
                &[],
                0
            ),
            Err(CachedFetchError::InvalidFetchTime)
        ));
        assert_eq!(offline.calls, 0);
        let mut forged = good();
        forged.replies.pop_back();
        forged.replies.push_back(
            HTML.replace(r#""revid":333"#, r#""revid":334"#)
                .into_bytes(),
        );
        assert!(matches!(
            fetch_page_cached(
                &mut forged,
                &mut cache,
                &key(),
                NetworkState::Online,
                &[],
                100
            ),
            Err(CachedFetchError::Fetch(_))
        ));
        assert_eq!(cache.get(&key(), 333).unwrap(), None);
    }

    #[test]
    fn revision_conflict_cannot_overwrite_cached_html() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        fetch_page_cached(
            &mut good(),
            &mut cache,
            &key(),
            NetworkState::Online,
            &[],
            100,
        )
        .unwrap();
        let mut forged = good();
        forged.replies.pop_back();
        forged.replies.push_back(
            HTML.replace("<p>Physics</p>", "<p>Changed</p>")
                .into_bytes(),
        );
        assert!(matches!(
            fetch_page_cached(
                &mut forged,
                &mut cache,
                &key(),
                NetworkState::Online,
                &[],
                200
            ),
            Err(CachedFetchError::Cache(HtmlCacheError::RevisionConflict))
        ));
        assert_eq!(
            cache.get(&key(), 333).unwrap().unwrap().html,
            "<p>Physics</p>"
        );
    }

    #[test]
    fn redirect_fetch_caches_target_page_id() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let title = r#"{"query":{"redirects":[{"from":"Physics intro","to":"Physics"}],"pages":[{"pageid":42,"ns":0,"title":"Physics"}]}}"#;
        let mut client = Fake {
            replies: VecDeque::from(vec![
                title.as_bytes().to_vec(),
                META.as_bytes().to_vec(),
                HTML.as_bytes().to_vec(),
            ]),
            calls: 0,
        };
        assert!(matches!(
            fetch_title_cached(
                &mut client,
                &mut cache,
                "enwiki",
                "Physics intro",
                NetworkState::Online,
                100,
            ),
            Ok(FetchOutcome::Rendered(_))
        ));
        assert_eq!(client.calls, 3);
        assert_eq!(cache.get(&key(), 333).unwrap().unwrap().title, "Physics");
    }
    #[test]
    fn upstream_revision_regression_fails_closed_without_cache_mutation() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        cache
            .put(&CachedHtml {
                key: key(),
                revision_id: 334,
                title: "Physics".into(),
                html: "<p>Newer version</p>".into(),
                fetched_at: 100,
            })
            .unwrap();
        let mut transport = good();
        assert!(matches!(
            fetch_page_cached(
                &mut transport,
                &mut cache,
                &key(),
                NetworkState::Online,
                &[],
                200
            ),
            Err(CachedFetchError::RevisionRegression {
                newest_cached: 334,
                incoming: 333,
            })
        ));
        assert_eq!(transport.calls, 2);
        assert_eq!(cache.get(&key(), 333).unwrap(), None);
        assert_eq!(
            cache.get(&key(), 334).unwrap().unwrap().html,
            "<p>Newer version</p>"
        );
    }

    #[test]
    fn newest_cached_revision_does_not_regress_after_older_refresh() {
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        let older = CachedHtml {
            key: key(),
            revision_id: 300,
            title: "Physics".into(),
            html: "<p>Old revision</p>".into(),
            fetched_at: 900,
        };
        let newer = CachedHtml {
            key: key(),
            revision_id: 333,
            title: "Physics".into(),
            html: "<p>New revision</p>".into(),
            fetched_at: 100,
        };
        cache.put(&newer).unwrap();
        cache.put(&older).unwrap();
        // Cache enumeration is deliberately fetch-time ordered, whereas
        // "latest revision" must use the upstream revision identity.
        assert_eq!(
            cache.list_cached_revisions(&key()).unwrap()[0].revision_id,
            300
        );
        let chosen = latest_temporary_version(&cache, &key()).unwrap().unwrap();
        assert_eq!(chosen.revision_id, 333);
        assert_eq!(chosen.location, ArticleLocation::TemporaryCache);
        assert!(!chosen.local_complete);
        cache.remove(&key(), 333).unwrap();
        assert_eq!(
            latest_temporary_version(&cache, &key())
                .unwrap()
                .unwrap()
                .revision_id,
            300
        );
    }

    #[test]
    fn cached_html_candidate_cannot_claim_offline_completeness() {
        use wiki_source::online_selection::{choose_article, ReaderSelection};
        let mut cache = RevisionHtmlCache::in_memory().unwrap();
        assert!(latest_temporary_version(&cache, &key()).unwrap().is_none());
        fetch_page_cached(
            &mut good(),
            &mut cache,
            &key(),
            NetworkState::Online,
            &[],
            100,
        )
        .unwrap();
        let cached = latest_temporary_version(&cache, &key()).unwrap().unwrap();
        assert_eq!(cached.key, key());
        assert_eq!(cached.revision_id, 333);
        assert_eq!(cached.location, ArticleLocation::TemporaryCache);
        assert!(!cached.local_complete);
        assert_eq!(
            choose_article(&key(), NetworkState::Offline, &[cached.clone()]),
            Ok(ReaderSelection::MissingOffline)
        );
        assert_eq!(
            choose_article(&key(), NetworkState::Online, &[cached]),
            Ok(ReaderSelection::FetchCurrent { fallback: None })
        );
    }
}
