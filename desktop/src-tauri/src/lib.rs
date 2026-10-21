//! Thin desktop Tauri adapter around the shared revision-safe Rust source API.
//! Host-only HTTPS transport uses system curl and never qualifies offline media.

use tauri::Manager;
use wiki_model::ArticleKey;
use wiki_store::user_state::{UserState, UserStateError};
use wiki_source::online_curl::CurlTransport;
use wiki_source::online_fetch::{fetch_page_id, fetch_title, FetchOutcome};
use wiki_source::online_search::{fetch_search, SearchOutcome};
use wiki_source::online_selection::NetworkState;

/// Search results are navigation identities only, never AI evidence.
#[tauri::command]
async fn search_wikipedia(query: String, project: String, offline: bool) -> Result<Vec<(u64, String)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_search(&mut client, &project, &query, 10, network)
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
async fn open_wikipedia_title(title: String, project: String, offline: bool) -> Result<(u64, u64, String, String), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_title(&mut client, &project, &title, network)
            .map_err(|error| format!("Wikipedia article fetch failed: {error:?}"))?;
        preview_response(outcome)
    })
    .await
    .map_err(|_| "Wikipedia article worker stopped".to_owned())?
}

/// Convert only authenticated, exact-revision rendered HTML into the preview DTO.
fn preview_response(outcome: FetchOutcome) -> Result<(u64, u64, String, String), String> {
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
}

/// Search hits carry page IDs. Re-resolving their mutable titles can select
/// a different article between search and click, so open the stable ID instead.
#[tauri::command]
async fn open_wikipedia_page(page_id: u64, project: String, offline: bool) -> Result<(u64, u64, String, String), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_page_id(&mut client, &project, page_id, network)
            .map_err(|error| format!("Wikipedia article fetch failed: {error:?}"))?;
        preview_response(outcome)
    })
    .await
    .map_err(|_| "Wikipedia article worker stopped".to_owned())?
}


fn open_private_bookmark_store(app: &tauri::AppHandle) -> Result<UserState, String> {
    let base = app.path().app_data_dir()
        .map_err(|_| "Cannot find the app data directory")?;
    std::fs::create_dir_all(&base)
        .map_err(|_| "Cannot create the app data directory")?;
    let private = base.join("reader-state");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        match std::fs::DirBuilder::new().mode(0o700).create(&private) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Cannot create private reader state".into()),
        }
        let metadata = std::fs::symlink_metadata(&private)
            .map_err(|_| "Cannot inspect private reader state")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err("Reader state directory is not private".into());
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&private)
            .map_err(|_| "Cannot create private reader state")?;
        let metadata = std::fs::symlink_metadata(&private)
            .map_err(|_| "Cannot inspect private reader state")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Reader state directory is unsafe".into());
        }
    }
    let database = private.join("user-state.sqlite");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        match std::fs::OpenOptions::new()
            .write(true).create_new(true).mode(0o600).open(&database)
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Cannot initialize private bookmark database".into()),
        }
        let metadata = std::fs::symlink_metadata(&database)
            .map_err(|_| "Cannot inspect bookmark database")?;
        if !metadata.is_file() || metadata.file_type().is_symlink()
            || metadata.nlink() != 1
        {
            return Err("Bookmark database path is unsafe".into());
        }
    }
    #[cfg(not(unix))]
    if let Ok(metadata) = std::fs::symlink_metadata(&database) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Bookmark database path is unsafe".into());
        }
    }
    UserState::open(&database).map_err(|_| "Cannot open bookmark database".to_owned())
}

fn toggle_saved_bookmark(
    state: &UserState,
    key: &ArticleKey,
    revision_id: u64,
) -> Result<bool, UserStateError> {
    key.validate().map_err(|_| UserStateError::InvalidValue)?;
    if revision_id == 0 || revision_id > i64::MAX as u64 {
        return Err(UserStateError::InvalidValue);
    }
    if state.bookmark(key)?.is_some() {
        state.remove_bookmark(key)?;
        Ok(false)
    } else {
        // Empty anchor means page-level bookmark; it is not an AI citation.
        state.set_bookmark(key, revision_id, "")?;
        Ok(true)
    }
}

/// Bookmark an exact page identity without saving text or media for offline use.
/// Bookmark rows live in a private SQLite database, separate from pack content.
#[tauri::command]
async fn toggle_reader_bookmark(
    app: tauri::AppHandle,
    project: String,
    page_id: u64,
    revision_id: u64,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = open_private_bookmark_store(&app)?;
        let key = ArticleKey { project, page_id };
        toggle_saved_bookmark(&state, &key, revision_id)
            .map_err(|_| "Invalid bookmark identity or storage operation".to_owned())
    })
    .await
    .map_err(|_| "Bookmark worker stopped".to_owned())?
}

#[cfg(test)]
mod bookmark_tests {
    use super::*;

    #[test]
    fn page_bookmarks_toggle_without_creating_offline_content() {
        let state = UserState::in_memory().unwrap();
        let article = ArticleKey {
            project: "enwiki".to_owned(),
            page_id: 42,
        };
        assert_eq!(toggle_saved_bookmark(&state, &article, 333).unwrap(), true);
        assert_eq!(state.bookmark(&article).unwrap(), Some((333, String::new())));
        assert_eq!(toggle_saved_bookmark(&state, &article, 334).unwrap(), false);
        assert_eq!(state.bookmark(&article).unwrap(), None);
        assert!(toggle_saved_bookmark(&state, &article, 0).is_err());
        assert!(toggle_saved_bookmark(&state, &article, i64::MAX as u64 + 1).is_err());
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            search_wikipedia,
            open_wikipedia_title,
            open_wikipedia_page,
            toggle_reader_bookmark
        ])
        .run(tauri::generate_context!())
        .expect("Foundation Wikipedia desktop failed to start");
}
