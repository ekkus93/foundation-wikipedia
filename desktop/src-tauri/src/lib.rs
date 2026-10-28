//! Thin desktop Tauri adapter around the shared revision-safe Rust source API.
//! Host-only HTTPS transport uses system curl and never qualifies offline media.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;
use wiki_model::ArticleKey;
use wiki_store::user_state::{UserState, UserStateError};
use wiki_source::online_cache::OnlineRevisionCache;
use wiki_source::online_curl::CurlTransport;
use wiki_source::online_fetch::{
    fetch_page_id, fetch_page_id_with_cache, fetch_title, fetch_title_with_cache, FetchFailure,
    FetchOutcome, MediaWikiTransport,
};
use wiki_source::online_search::{fetch_search, SearchOutcome};
use wiki_source::online_selection::NetworkState;

/// Shared across desktop commands for this application process only. A cached
/// sanitized HTML preview is never an installed/offline-complete article.
#[derive(Clone, Default)]
struct DesktopOnlineCache(Arc<Mutex<OnlineRevisionCache>>, Arc<AtomicU64>);

/// Serialize privacy-sensitive writes with deletion and reject operations
/// queued before a privacy reset. The generation is captured at IPC entry,
/// not when a delayed blocking worker finally starts.
#[derive(Clone, Default)]
struct DesktopPersonalDataGate {
    lock: Arc<Mutex<()>>,
    generation: Arc<AtomicU64>,
    visit_generation: Arc<AtomicU64>,
}

impl DesktopPersonalDataGate {
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn visit_generation(&self) -> u64 {
        self.visit_generation.load(Ordering::Acquire)
    }

    fn write<T>(
        &self,
        expected_generation: u64,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Personal reading data lock unavailable".to_owned())?;
        if self.generation() != expected_generation {
            return Err("Personal reading data was cleared; stale write rejected".to_owned());
        }
        action()
    }

    fn write_visit<T>(
        &self,
        expected_generation: u64,
        expected_visit_generation: u64,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.write(expected_generation, || {
            if self.visit_generation() != expected_visit_generation {
                return Err("Reading history was cleared; stale visit rejected".to_owned());
            }
            action()
        })
    }

    fn revoke_queued_visits<T>(
        &self,
        expected_generation: u64,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        // Check the personal-data epoch under the visit-writer lock. Stale
        // clear/disable requests must not revoke post-reset visits.
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Personal reading data lock unavailable".to_owned())?;
        if self.generation() != expected_generation {
            return Err("Personal reading data was cleared; stale write rejected".to_owned());
        }
        // Older visits either finished before this operation or will observe
        // the new epoch afterward. Bookmarks are unaffected.
        self.visit_generation.fetch_add(1, Ordering::AcqRel);
        action()
    }

    fn reset<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        // Invalidate queued workers before waiting for any active write.
        // A worker already holding the lock finishes before deletion begins.
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.visit_generation.fetch_add(1, Ordering::AcqRel);
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Personal reading data lock unavailable".to_owned())?;
        action()
    }
}

/// Avoid serializing independent HTTP operations behind the cache lock. If a
/// concurrent request owns the lock (or it was poisoned), use the uncached
/// revision-safe path. An offline command never inspects the online cache.
fn fetch_title_preview<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    title: &str,
    network: NetworkState,
    cache: &DesktopOnlineCache,
    expected_generation: u64,
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    if network == NetworkState::Offline {
        return fetch_title(client, project, title, network);
    }
    // Requests queued before a privacy reset may start afterward. Never let
    // an older generation refill the cleared in-memory HTML cache.
    if cache.1.load(Ordering::Acquire) != expected_generation {
        return fetch_title(client, project, title, network);
    }
    match cache.0.try_lock() {
        Ok(mut locked) if cache.1.load(Ordering::Acquire) == expected_generation => {
            fetch_title_with_cache(client, project, title, network, &[], &mut locked)
        }
        _ => fetch_title(client, project, title, network),
    }
}

fn fetch_page_preview<T: MediaWikiTransport>(
    client: &mut T,
    project: &str,
    page_id: u64,
    network: NetworkState,
    cache: &DesktopOnlineCache,
    expected_generation: u64,
) -> Result<FetchOutcome, FetchFailure<T::Error>> {
    if network == NetworkState::Offline {
        return fetch_page_id(client, project, page_id, network);
    }
    if cache.1.load(Ordering::Acquire) != expected_generation {
        return fetch_page_id(client, project, page_id, network);
    }
    match cache.0.try_lock() {
        Ok(mut locked) if cache.1.load(Ordering::Acquire) == expected_generation => {
            fetch_page_id_with_cache(client, project, page_id, network, &[], &mut locked)
        }
        _ => fetch_page_id(client, project, page_id, network),
    }
}

/// Increment generation before locking: an old queued request cannot refill
/// the erased cache even if its worker begins after the reset completes.
fn clear_online_preview_cache(cache: &DesktopOnlineCache) -> Result<(), String> {
    cache.1.fetch_add(1, Ordering::AcqRel);
    let mut locked = cache.0.lock().map_err(|_| "Cannot clear online preview cache")?;
    *locked = OnlineRevisionCache::default();
    Ok(())
}

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
            // The desktop adapter has not yet been wired to a verified local catalog.
            // Never present an offline result without its required media objects.
            SearchOutcome::LocalResults(_) => {
                Err("Offline search integration is not yet available".to_owned())
            }
        }
    })
    .await
    .map_err(|_| "Wikipedia search worker stopped".to_owned())?
}

/// Return sanitized HTML tied to the exact Wikimedia page and revision.
/// This is an online-only, text-preview bridge, not an offline-ready article.
#[tauri::command]
async fn open_wikipedia_title(
    title: String,
    project: String,
    offline: bool,
    cache: tauri::State<'_, DesktopOnlineCache>,
) -> Result<(u64, u64, String, String), String> {
    let generation = cache.1.load(Ordering::Acquire);
    let cache = (*cache).clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_title_preview(&mut client, &project, &title, network, &cache, generation)
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
async fn open_wikipedia_page(
    page_id: u64,
    project: String,
    offline: bool,
    cache: tauri::State<'_, DesktopOnlineCache>,
) -> Result<(u64, u64, String, String), String> {
    let generation = cache.1.load(Ordering::Acquire);
    let cache = (*cache).clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = CurlTransport::new();
        let network = if offline { NetworkState::Offline } else { NetworkState::Online };
        let outcome = fetch_page_preview(&mut client, &project, page_id, network, &cache, generation)
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
    gate: tauri::State<'_, DesktopPersonalDataGate>,
    project: String,
    page_id: u64,
    revision_id: u64,
) -> Result<bool, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.write(generation, || {
            let state = open_private_bookmark_store(&app)?;
            let key = ArticleKey { project, page_id };
            toggle_saved_bookmark(&state, &key, revision_id)
                .map_err(|_| "Invalid bookmark identity or storage operation".to_owned())
        })
    })
    .await
    .map_err(|_| "Bookmark worker stopped".to_owned())?
}


fn saved_bookmark_rows(state: &UserState) -> Result<Vec<(String, u64, u64)>, UserStateError> {
    Ok(state
        .list_bookmarks(100)?
        .into_iter()
        .map(|item| (item.key.project, item.key.page_id, item.revision_id))
        .collect())
}

/// Retrieve bounded saved identities only; no article HTML or offline media.
#[tauri::command]
async fn list_reader_bookmarks(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
) -> Result<Vec<(String, u64, u64)>, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.write(generation, || {
            let state = open_private_bookmark_store(&app)?;
            saved_bookmark_rows(&state)
                .map_err(|_| "Cannot read saved page identities".to_owned())
        })
    })
    .await
    .map_err(|_| "Bookmark reader stopped".to_owned())?
}

/// Remove a bookmark without deleting an installed pack or cached media.
#[tauri::command]
async fn remove_reader_bookmark(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
    project: String,
    page_id: u64,
) -> Result<bool, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.write(generation, || {
            let state = open_private_bookmark_store(&app)?;
            let key = ArticleKey { project, page_id };
            state.remove_bookmark(&key)
                .map_err(|_| "Cannot remove bookmark identity".to_owned())
        })
    })
    .await
    .map_err(|_| "Bookmark removal worker stopped".to_owned())?
}

/// Erase personal reading activity in one SQLite transaction without touching
/// pack ownership, builder jobs or ordinary settings. The UI requires an explicit
/// second confirmation before invoking this destructive operation.
#[tauri::command]
async fn clear_reader_personal_data(
    app: tauri::AppHandle,
    cache: tauri::State<'_, DesktopOnlineCache>,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
) -> Result<(), String> {
    let cache = (*cache).clone();
    let gate = (*gate).clone();
    tauri::async_runtime::spawn_blocking(move || {
        gate.reset(|| {
            let mut state = open_private_bookmark_store(&app)?;
            state.clear_personal_reading_data()
                .map_err(|_| "Cannot clear personal reading data".to_owned())?;
            clear_online_preview_cache(&cache)
        })
    })
    .await
    .map_err(|_| "Personal reading data worker stopped".to_owned())?
}


fn reader_history_rows(state: &UserState) -> Result<Vec<(String, u64, u64, i64)>, UserStateError> {
    Ok(state.recent_history(50)?.into_iter()
        .map(|entry| (entry.key.project, entry.key.page_id, entry.revision_id, entry.visited_at))
        .collect())
}

/// Visit storage is strictly opt-in and records only an exact verified article
/// identity following a user-initiated successful navigation. No HTML is stored.
#[tauri::command]
async fn record_reader_visit(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
    project: String,
    page_id: u64,
    revision_id: u64,
) -> Result<bool, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    let visit_generation = gate.visit_generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.write_visit(generation, visit_generation, || {
            let state = open_private_bookmark_store(&app)?;
            let key = ArticleKey { project, page_id };
            let seconds = SystemTime::now().duration_since(UNIX_EPOCH)
                .map_err(|_| "System clock is invalid")?.as_secs();
            let timestamp = i64::try_from(seconds).map_err(|_| "System clock is invalid")?;
            state.record_visit(&key, revision_id, timestamp)
                .map_err(|_| "Cannot record reading history".to_owned())
        })
    })
    .await
    .map_err(|_| "Reading history worker stopped".to_owned())?
}

#[tauri::command]
async fn reader_history_enabled(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
) -> Result<bool, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.write(generation, || {
            open_private_bookmark_store(&app)?.history_enabled()
                .map_err(|_| "Cannot read history preference".to_owned())
        })
    })
    .await
    .map_err(|_| "History preference worker stopped".to_owned())?
}

#[tauri::command]
async fn set_reader_history_enabled(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
    enabled: bool,
) -> Result<(), String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        let update = || {
            open_private_bookmark_store(&app)?.set_history_enabled(enabled)
                .map_err(|_| "Cannot save history preference".to_owned())
        };
        if enabled {
            gate.write(generation, update)
        } else {
            // Revoking consent also fences visits queued while it was enabled.
            gate.revoke_queued_visits(generation, update)
        }
    })
    .await
    .map_err(|_| "History preference worker stopped".to_owned())?
}

#[tauri::command]
async fn list_reader_history(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
) -> Result<Vec<(String, u64, u64, i64)>, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.write(generation, || {
            let state = open_private_bookmark_store(&app)?;
            reader_history_rows(&state).map_err(|_| "Cannot load reading history".to_owned())
        })
    })
    .await
    .map_err(|_| "Reading history worker stopped".to_owned())?
}

#[tauri::command]
async fn clear_reader_history(
    app: tauri::AppHandle,
    gate: tauri::State<'_, DesktopPersonalDataGate>,
) -> Result<usize, String> {
    let gate = (*gate).clone();
    let generation = gate.generation();
    tauri::async_runtime::spawn_blocking(move || {
        gate.revoke_queued_visits(generation, || {
            open_private_bookmark_store(&app)?.clear_history()
                .map_err(|_| "Cannot clear reading history".to_owned())
        })
    })
    .await
    .map_err(|_| "Reading history worker stopped".to_owned())?
}

fn validated_reader_theme(state: &UserState) -> Result<Option<String>, UserStateError> {
    match state.setting("reading.theme")? {
        None => Ok(None),
        Some(value) if matches!(value.as_str(), "system" | "light" | "dark") => Ok(Some(value)),
        _ => Err(UserStateError::InvalidValue),
    }
}

/// Persist non-secret appearance settings in the same private user-state DB.
#[tauri::command]
async fn read_reader_theme(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = open_private_bookmark_store(&app)?;
        validated_reader_theme(&state).map_err(|_| "Invalid saved appearance preference".to_owned())
    })
    .await
    .map_err(|_| "Appearance preference worker stopped".to_owned())?
}

#[tauri::command]
async fn set_reader_theme(app: tauri::AppHandle, theme: String) -> Result<(), String> {
    if !matches!(theme.as_str(), "system" | "light" | "dark") {
        return Err("Unsupported appearance preference".to_owned());
    }
    tauri::async_runtime::spawn_blocking(move || {
        open_private_bookmark_store(&app)?.set_setting("reading.theme", &theme)
            .map_err(|_| "Cannot save appearance preference".to_owned())
    })
    .await
    .map_err(|_| "Appearance preference worker stopped".to_owned())?
}


fn validated_reader_layout(state: &UserState) -> Result<(String, String), UserStateError> {
    let text_size = state.setting("reading.text_size")?
        .unwrap_or_else(|| "standard".to_owned());
    let reading_width = state.setting("reading.width")?
        .unwrap_or_else(|| "standard".to_owned());
    if !matches!(text_size.as_str(), "small" | "standard" | "large")
        || !matches!(reading_width.as_str(), "narrow" | "standard" | "wide")
    {
        return Err(UserStateError::InvalidValue);
    }
    Ok((text_size, reading_width))
}

/// These preferences only affect app-controlled presentation, never untrusted
/// article HTML, and contain no provider credentials or personal article data.
#[tauri::command]

async fn read_reader_layout(app: tauri::AppHandle) -> Result<(String, String), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = open_private_bookmark_store(&app)?;
        validated_reader_layout(&state)
            .map_err(|_| "Invalid saved reading layout".to_owned())
    })
    .await
    .map_err(|_| "Reading layout worker stopped".to_owned())?
}

#[tauri::command]
async fn set_reader_text_size(app: tauri::AppHandle, text_size: String) -> Result<(), String> {
    if !matches!(text_size.as_str(), "small" | "standard" | "large") {
        return Err("Unsupported reading text size".to_owned());
    }
    tauri::async_runtime::spawn_blocking(move || {
        open_private_bookmark_store(&app)?.set_setting("reading.text_size", &text_size)
            .map_err(|_| "Cannot save reading text size".to_owned())
    })
    .await
    .map_err(|_| "Reading text size worker stopped".to_owned())?
}

#[tauri::command]
async fn set_reader_width(app: tauri::AppHandle, reading_width: String) -> Result<(), String> {
    if !matches!(reading_width.as_str(), "narrow" | "standard" | "wide") {
        return Err("Unsupported reading width".to_owned());
    }
    tauri::async_runtime::spawn_blocking(move || {
        open_private_bookmark_store(&app)?.set_setting("reading.width", &reading_width)
            .map_err(|_| "Cannot save reading width".to_owned())
    })
    .await
    .map_err(|_| "Reading width worker stopped".to_owned())?
}

#[cfg(test)]
mod online_preview_cache_tests {
    use super::*;
    use std::collections::VecDeque;

    const METADATA: &str = r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Physics","revisions":[{"revid":333,"timestamp":"2026-10-13T10:45:00Z"}]}]}}"#;
    const RENDERED: &str =
        r#"{"parse":{"pageid":42,"revid":333,"title":"Physics","text":"<p>Physics</p>"}}"#;
    const TITLE: &str =
        r#"{"query":{"pages":[{"pageid":42,"ns":0,"title":"Physics"}]}}"#;

    #[derive(Default)]
    struct FakeTransport {
        responses: VecDeque<Vec<u8>>,
        calls: usize,
    }

    impl MediaWikiTransport for FakeTransport {
        type Error = &'static str;

        fn get(&mut self, _url: &str, _max_bytes: usize) -> Result<Vec<u8>, Self::Error> {
            self.calls += 1;
            self.responses.pop_front().ok_or("unexpected HTTP request")
        }
    }

    #[test]
    fn desktop_page_and_title_share_revision_pinned_html_but_not_metadata() {
        let cache = DesktopOnlineCache::default();
        let another_command = cache.clone();
        let mut transport = FakeTransport::default();
        transport.responses.extend([
            METADATA.as_bytes().to_vec(),
            RENDERED.as_bytes().to_vec(),
        ]);
        let first = fetch_page_preview(
            &mut transport,
            "enwiki",
            42,
            NetworkState::Online,
            &cache,
            cache.1.load(Ordering::Acquire),
        )
        .unwrap();
        assert_eq!(transport.calls, 2);

        // A different Tauri command shares the in-process cache but still
        // resolves the title and current revision from the network.
        transport.responses.extend([
            TITLE.as_bytes().to_vec(),
            METADATA.as_bytes().to_vec(),
        ]);
        let second = fetch_title_preview(
            &mut transport,
            "enwiki",
            "Physics",
            NetworkState::Online,
            &another_command,
            another_command.1.load(Ordering::Acquire),
        )
        .unwrap();
        assert_eq!(second, first);
        assert_eq!(transport.calls, 4);
        assert_eq!(cache.0.lock().unwrap().len(), 1);

        // An offline request must not consume cached online HTML or call HTTP.
        assert_eq!(
            fetch_page_preview(
                &mut transport,
                "enwiki",
                42,
                NetworkState::Offline,
                &cache,
                cache.1.load(Ordering::Acquire),
            ),
            Ok(FetchOutcome::MissingOffline)
        );
        assert_eq!(
            fetch_title_preview(
                &mut transport,
                "enwiki",
                "Physics",
                NetworkState::Offline,
                &cache,
                cache.1.load(Ordering::Acquire),
            ),
            Ok(FetchOutcome::MissingOffline)
        );
        assert_eq!(transport.calls, 4);

        // Privacy reset drops recently opened page identities from RAM too.
        clear_online_preview_cache(&cache).unwrap();
        assert!(another_command.0.lock().unwrap().is_empty());
        // A queued old request can return an online preview but cannot
        // repopulate RAM after the explicit privacy reset.
        let stale_generation = cache.1.load(Ordering::Acquire).saturating_sub(1);
        transport.responses.extend([
            METADATA.as_bytes().to_vec(),
            RENDERED.as_bytes().to_vec(),
        ]);
        assert!(matches!(
            fetch_page_preview(
                &mut transport,
                "enwiki",
                42,
                NetworkState::Online,
                &cache,
                stale_generation,
            ),
            Ok(FetchOutcome::Rendered(_))
        ));
        assert!(cache.0.lock().unwrap().is_empty());
        transport.responses.extend([
            METADATA.as_bytes().to_vec(),
            RENDERED.as_bytes().to_vec(),
        ]);
        assert_eq!(
            fetch_page_preview(
                &mut transport,
                "enwiki",
                42,
                NetworkState::Online,
                &cache,
                cache.1.load(Ordering::Acquire),
            ),
            Ok(first)
        );
        assert_eq!(transport.calls, 8);
    }

    #[test]
    fn concurrent_cache_ownership_falls_back_without_waiting_or_reusing_html() {
        let cache = DesktopOnlineCache::default();
        let held = cache.0.lock().unwrap();
        let mut transport = FakeTransport::default();
        transport.responses.extend([
            METADATA.as_bytes().to_vec(),
            RENDERED.as_bytes().to_vec(),
        ]);
        assert!(matches!(
            fetch_page_preview(
                &mut transport,
                "enwiki",
                42,
                NetworkState::Online,
                &cache,
                cache.1.load(Ordering::Acquire),
            ),
            Ok(FetchOutcome::Rendered(_))
        ));
        assert_eq!(transport.calls, 2);
        assert!(held.is_empty());
    }
}

#[cfg(test)]
mod bookmark_tests {
    use super::*;

    #[test]
    fn visit_history_is_opt_in_bounded_and_independent_of_bookmarks() {
        let state = UserState::in_memory().unwrap();
        let key = ArticleKey { project: "enwiki".to_owned(), page_id: 42 };
        state.set_bookmark(&key, 333, "").unwrap();
        assert!(!state.history_enabled().unwrap());
        assert!(!state.record_visit(&key, 333, 1700000000).unwrap());
        assert!(reader_history_rows(&state).unwrap().is_empty());
        state.set_history_enabled(true).unwrap();
        assert!(state.record_visit(&key, 333, 1700000001).unwrap());
        assert!(state.record_visit(&key, 334, 1700000002).unwrap());
        assert_eq!(reader_history_rows(&state).unwrap(),
            vec![("enwiki".to_owned(), 42, 334, 1700000002),
                 ("enwiki".to_owned(), 42, 333, 1700000001)]);
        state.set_history_enabled(false).unwrap();
        assert!(!state.record_visit(&key, 335, 1700000003).unwrap());
        assert_eq!(state.clear_history().unwrap(), 2);
        assert!(reader_history_rows(&state).unwrap().is_empty());
        assert_eq!(state.bookmark(&key).unwrap(), Some((333, String::new())));
    }

    #[test]
    fn saved_theme_is_typed_and_rejects_corrupt_preferences() {
        let state = UserState::in_memory().unwrap();
        assert_eq!(validated_reader_theme(&state).unwrap(), None);
        for theme in ["system", "light", "dark"] {
            state.set_setting("reading.theme", theme).unwrap();
            assert_eq!(validated_reader_theme(&state).unwrap().as_deref(), Some(theme));
        }
        state.set_setting("reading.theme", "invalid").unwrap();
        assert!(matches!(validated_reader_theme(&state), Err(UserStateError::InvalidValue)));
    }


    #[test]
    fn reading_layout_is_defaulted_validated_and_preserves_independent_choices() {
        let state = UserState::in_memory().unwrap();
        assert_eq!(validated_reader_layout(&state).unwrap(), ("standard".to_owned(), "standard".to_owned()));
        for size in ["small", "standard", "large"] {
            state.set_setting("reading.text_size", size).unwrap();
            assert_eq!(validated_reader_layout(&state).unwrap().0, size);
        }
        for width in ["narrow", "standard", "wide"] {
            state.set_setting("reading.width", width).unwrap();
            assert_eq!(validated_reader_layout(&state).unwrap().1, width);
        }
        state.set_setting("reading.text_size", "42px; color: red").unwrap();
        assert!(matches!(validated_reader_layout(&state), Err(UserStateError::InvalidValue)));
        state.set_setting("reading.text_size", "standard").unwrap();
        state.set_setting("reading.width", "calc(100vh)").unwrap();
        assert!(matches!(validated_reader_layout(&state), Err(UserStateError::InvalidValue)));
    }

    #[test]
    fn reading_layout_survives_restart_without_changing_other_user_state() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-layout-persistence-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("user-state.sqlite");
        {
            let state = UserState::open(&db).unwrap();
            state.set_setting("reading.text_size", "large").unwrap();
            state.set_setting("reading.width", "narrow").unwrap();
            state.set_setting("reading.theme", "dark").unwrap();
        }
        {
            let state = UserState::open(&db).unwrap();
            assert_eq!(
                validated_reader_layout(&state).unwrap(),
                ("large".to_owned(), "narrow".to_owned())
            );
            assert_eq!(validated_reader_theme(&state).unwrap(), Some("dark".to_owned()));
            state.set_setting("reading.text_size", "small").unwrap();
        }
        {
            let state = UserState::open(&db).unwrap();
            assert_eq!(
                validated_reader_layout(&state).unwrap(),
                ("small".to_owned(), "narrow".to_owned())
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn privacy_reset_fences_queued_bookmark_and_visit_writes() {
        let gate = DesktopPersonalDataGate::default();
        let queued_generation = gate.generation();
        let queued_visit_generation = gate.visit_generation();
        let mut state = UserState::in_memory().unwrap();
        let key = ArticleKey {
            project: "enwiki".to_owned(),
            page_id: 42,
        };
        state.set_history_enabled(true).unwrap();
        gate.write_visit(queued_generation, queued_visit_generation, || {
            state.record_visit(&key, 333, 1_700_000_000)
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        gate.write(queued_generation, || {
            toggle_saved_bookmark(&state, &key, 333)
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        gate.reset(|| {
            state.clear_personal_reading_data()
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        assert!(reader_history_rows(&state).unwrap().is_empty());
        assert!(state.bookmark(&key).unwrap().is_none());

        // Workers queued before reset cannot reintroduce cleared activity.
        assert!(gate
            .write_visit(queued_generation, queued_visit_generation, || {
                state.record_visit(&key, 334, 1_700_000_001)
                    .map_err(|error| format!("{error:?}"))
            })
            .is_err());
        assert!(gate
            .write(queued_generation, || {
                toggle_saved_bookmark(&state, &key, 334)
                    .map_err(|error| format!("{error:?}"))
            })
            .is_err());
        assert!(reader_history_rows(&state).unwrap().is_empty());
        assert!(state.bookmark(&key).unwrap().is_none());

        // A deliberate post-reset action is still permitted.
        let current_generation = gate.generation();
        assert_ne!(queued_generation, current_generation);
        assert!(gate
            .write(current_generation, || {
                toggle_saved_bookmark(&state, &key, 335)
                    .map_err(|error| format!("{error:?}"))
            })
            .unwrap());
        assert_eq!(state.bookmark(&key).unwrap(), Some((335, String::new())));
    }

    #[test]
    fn clearing_history_fences_queued_visits_without_affecting_bookmarks() {
        let gate = DesktopPersonalDataGate::default();
        let mut state = UserState::in_memory().unwrap();
        let key = ArticleKey {
            project: "enwiki".to_owned(),
            page_id: 42,
        };
        state.set_history_enabled(true).unwrap();
        let generation = gate.generation();
        let old_visit_generation = gate.visit_generation();
        gate.write_visit(generation, old_visit_generation, || {
            state.record_visit(&key, 333, 1_700_000_000)
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        gate.write(generation, || {
            toggle_saved_bookmark(&state, &key, 333)
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        gate.revoke_queued_visits(generation, || {
            state.clear_history().map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        assert!(reader_history_rows(&state).unwrap().is_empty());
        assert_eq!(state.bookmark(&key).unwrap(), Some((333, String::new())));

        assert!(gate
            .write_visit(generation, old_visit_generation, || {
                state.record_visit(&key, 334, 1_700_000_001)
                    .map_err(|error| format!("{error:?}"))
            })
            .is_err());
        assert!(reader_history_rows(&state).unwrap().is_empty());
        // A fresh visit is allowed; history-only deletion must not invalidate
        // independently queued bookmark operations.
        gate.write_visit(generation, gate.visit_generation(), || {
            state.record_visit(&key, 335, 1_700_000_002)
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        assert_eq!(reader_history_rows(&state).unwrap().len(), 1);
        assert!(gate.write(generation, || Ok(())).is_ok());
    }

    #[test]
    fn disabling_history_fences_queued_visits_even_after_reenable() {
        let gate = DesktopPersonalDataGate::default();
        let state = UserState::in_memory().unwrap();
        let key = ArticleKey {
            project: "enwiki".to_owned(),
            page_id: 42,
        };
        let generation = gate.generation();
        gate.write(generation, || {
            state.set_history_enabled(true).map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        let stale_visit_generation = gate.visit_generation();
        gate.revoke_queued_visits(generation, || {
            state.set_history_enabled(false).map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        assert!(!state.history_enabled().unwrap());
        // Re-enabling consent must not resurrect visits queued before opt-out.
        gate.write(generation, || {
            state.set_history_enabled(true).map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        assert!(gate
            .write_visit(generation, stale_visit_generation, || {
                state.record_visit(&key, 333, 1_700_000_000)
                    .map_err(|error| format!("{error:?}"))
            })
            .is_err());
        assert!(reader_history_rows(&state).unwrap().is_empty());
        assert!(gate
            .write_visit(generation, gate.visit_generation(), || {
                state.record_visit(&key, 334, 1_700_000_001)
                    .map_err(|error| format!("{error:?}"))
            })
            .unwrap());
    }

    #[test]
    fn stale_history_clear_cannot_invalidate_new_privacy_epoch() {
        let gate = DesktopPersonalDataGate::default();
        let mut state = UserState::in_memory().unwrap();
        let stale_generation = gate.generation();
        gate.reset(|| {
            state.clear_personal_reading_data()
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        let new_generation = gate.generation();
        let new_visit_generation = gate.visit_generation();
        assert!(gate
            .revoke_queued_visits(stale_generation, || {
                state.clear_history().map_err(|error| format!("{error:?}"))
            })
            .is_err());
        assert_eq!(gate.visit_generation(), new_visit_generation);
        assert!(gate
            .revoke_queued_visits(stale_generation, || {
                state.set_history_enabled(false)
                    .map_err(|error| format!("{error:?}"))
            })
            .is_err());
        assert_eq!(gate.visit_generation(), new_visit_generation);
        assert!(gate.write(new_generation, || Ok(())).is_ok());
    }

    #[test]
    fn stale_personal_data_reads_are_rejected_after_reset() {
        let gate = DesktopPersonalDataGate::default();
        let generation = gate.generation();
        let mut state = UserState::in_memory().unwrap();
        gate.reset(|| {
            state.clear_personal_reading_data()
                .map_err(|error| format!("{error:?}"))
        })
        .unwrap();
        assert!(gate.write(generation, || reader_history_rows(&state)
            .map_err(|error| format!("{error:?}"))).is_err());
        assert!(gate.write(generation, || saved_bookmark_rows(&state)
            .map_err(|error| format!("{error:?}"))).is_err());
    }

    #[test]
    fn page_bookmarks_toggle_without_creating_offline_content() {
        let state = UserState::in_memory().unwrap();
        let article = ArticleKey {
            project: "enwiki".to_owned(),
            page_id: 42,
        };
        assert_eq!(toggle_saved_bookmark(&state, &article, 333).unwrap(), true);
        assert_eq!(state.bookmark(&article).unwrap(), Some((333, String::new())));
        assert_eq!(saved_bookmark_rows(&state).unwrap(), vec![("enwiki".to_owned(), 42, 333)]);
        assert_eq!(toggle_saved_bookmark(&state, &article, 334).unwrap(), false);
        assert!(saved_bookmark_rows(&state).unwrap().is_empty());
        assert_eq!(state.bookmark(&article).unwrap(), None);
        assert!(toggle_saved_bookmark(&state, &article, 0).is_err());
        assert!(toggle_saved_bookmark(&state, &article, i64::MAX as u64 + 1).is_err());
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(DesktopOnlineCache::default())
        .manage(DesktopPersonalDataGate::default())
        .invoke_handler(tauri::generate_handler![
            search_wikipedia,
            open_wikipedia_title,
            open_wikipedia_page,
            toggle_reader_bookmark,
            list_reader_bookmarks,
            remove_reader_bookmark,
            clear_reader_personal_data,
            record_reader_visit,
            reader_history_enabled,
            set_reader_history_enabled,
            list_reader_history,
            clear_reader_history,
            read_reader_theme,
            set_reader_theme,
            read_reader_layout,
            set_reader_text_size,
            set_reader_width
        ])
        .run(tauri::generate_context!())
        .expect("Foundation Wikipedia desktop failed to start");
}
