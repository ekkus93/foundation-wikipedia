//! Thin desktop Tauri adapter around the shared revision-safe Rust source API.
//! Host-only HTTPS transport uses system curl and never qualifies offline media.

use wiki_source::online_curl::CurlTransport;
use wiki_source::online_fetch::{fetch_title, FetchOutcome};
use wiki_source::online_search::{fetch_search, SearchOutcome};
use wiki_source::online_selection::NetworkState;

/// Search results are navigation identities only, never AI evidence.
#[tauri::command]
async fn search_wikipedia(query: String, offline: bool) -> Result<Vec<(u64, String)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_search(&mut client, "enwiki", &query, 10, network)
            .map_err(|error| format!("Wikipedia search failed: {error:?}"))?;
        match outcome {
            SearchOutcome::Results(hits) => Ok(hits
                .into_iter()
                .map(|hit| (hit.key.page_id, hit.title))
                .collect()),
            SearchOutcome::OfflineNeedsLocalIndex => {
                Err("Offline search requires an installed local index; no network request was made".to_owned())
            }
        }
    })
    .await
    .map_err(|_| "Wikipedia search worker stopped".to_owned())?
}

/// Return sanitized HTML tied to the exact Wikimedia page and revision.
/// This is an online-only, text-preview bridge, not an offline-ready article.
#[tauri::command]
async fn open_wikipedia_title(title: String, offline: bool) -> Result<(u64, u64, String, String), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_title(&mut client, "enwiki", &title, network)
            .map_err(|error| format!("Wikipedia article fetch failed: {error:?}"))?;
        match outcome {
            FetchOutcome::Rendered(article) => Ok((
                article.revision.key.page_id,
                article.revision.revision_id,
                article.revision.title,
                article.html,
            )),
            FetchOutcome::MissingOffline => Err("Article is not installed for offline reading; no network request was made".to_owned()),
            _ => Err("No current online article is available".to_owned()),
        }
    })
    .await
    .map_err(|_| "Wikipedia article worker stopped".to_owned())?
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            search_wikipedia,
            open_wikipedia_title
        ])
        .run(tauri::generate_context!())
        .expect("Foundation Wikipedia desktop failed to start");
}
