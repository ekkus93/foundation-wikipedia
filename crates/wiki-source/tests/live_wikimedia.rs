//! Opt-in live acceptance for the real certificate-verifying host transport.
//! These tests are ignored by ordinary CI because Wikimedia and DNS are external.
//! Run explicitly with: cargo test -p wiki-source --test live_wikimedia -- --ignored

#![cfg(not(target_os = "android"))]

use wiki_source::online_curl::CurlTransport;
use wiki_source::online_fetch::{fetch_article, fetch_title, FetchOutcome};
use wiki_source::online_search::{fetch_search, SearchOutcome};
use wiki_source::online_selection::NetworkState;

#[test]
#[ignore = "requires real Wikimedia HTTPS, system curl, and external network access"]
fn official_search_returns_mainspace_navigation_identities() {
    let mut client = CurlTransport::new();
    let response = fetch_search(&mut client, "enwiki", "Physics", 5, NetworkState::Online)
        .expect("official HTTPS search should return valid MediaWiki JSON");
    let SearchOutcome::Results(hits) = response else {
        panic!("explicit online search must not report an offline index state");
    };
    assert!(!hits.is_empty(), "expected nonempty Wikipedia search results");
    for hit in hits {
        assert_eq!(hit.key.project, "enwiki");
        assert!(hit.key.page_id > 0);
        assert_eq!(hit.namespace, 0);
        assert!(!hit.title.is_empty());
    }
}

#[test]
#[ignore = "requires real Wikimedia HTTPS, system curl, and external network access"]
fn official_title_and_page_id_fetch_use_exact_rendered_revisions() {
    let mut client = CurlTransport::new();
    let response = fetch_title(&mut client, "enwiki", "Physics", NetworkState::Online)
        .expect("official title resolution, metadata, and pinned render should succeed");
    let FetchOutcome::Rendered(article) = response else {
        panic!("explicit online title fetch should return a rendered article");
    };
    assert_eq!(article.revision.key.project, "enwiki");
    assert!(article.revision.key.page_id > 0);
    assert_eq!(article.revision.title, "Physics");
    assert!(article.revision.revision_id > 0);
    assert!(!article.html.trim().is_empty());

    // A second current-revision read may race a Wikipedia edit; only the
    // source adapter's within-request oldid join is guaranteed to be exact.
    let page_response = fetch_article(
        &mut client,
        &article.revision.key,
        NetworkState::Online,
        &[],
    )
    .expect("official page-ID metadata and pinned render should succeed");
    let FetchOutcome::Rendered(by_id) = page_response else {
        panic!("explicit online page-ID fetch should return a rendered article");
    };
    assert_eq!(by_id.revision.key, article.revision.key);
    assert!(by_id.revision.revision_id > 0);
    assert!(!by_id.html.trim().is_empty());
}
