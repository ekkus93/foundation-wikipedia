//! MOB-004: the Android reader's bookmarks and opt-in reading history, kept
//! in the app-private user-state database (`wiki_store::user_state`) that
//! chat history also uses. Every write checks the durable privacy
//! generation in the same SQLite statement, as the desktop does, so a write
//! raced by "clear" or by turning history off cannot bring data back.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use wiki_model::ArticleKey;
use wiki_store::user_state::{UserState, UserStateError};

use crate::android::OfflineLibrary;

/// Bounded like the desktop panel.
const MAX_BOOKMARKS: usize = 100;
/// Reading positions are fractions of the page in millionths.
const POSITION_SCALE: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum ReaderStateError {
    /// An identifier or path was rejected before any write.
    InvalidInput,
    /// The database could not be opened, read or written.
    Storage,
    /// Saved data was cleared (or history turned off) meanwhile; nothing
    /// was written. Reload and try again.
    Changed,
}

impl std::fmt::Display for ReaderStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid reader state input",
            Self::Storage => "reader state storage failed",
            Self::Changed => "saved reader data changed",
        })
    }
}

fn store_error(error: UserStateError) -> ReaderStateError {
    match error {
        UserStateError::InvalidValue => ReaderStateError::InvalidInput,
        _ => ReaderStateError::Storage,
    }
}

/// One saved article: a bookmark or a history visit.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SavedPage {
    pub project: String,
    pub page_id: u64,
    /// The revision that was shown when it was saved.
    pub revision_id: u64,
    /// From the active pack; `None` when that pack does not have the page.
    pub title: Option<String>,
    /// Seconds since the Unix epoch, for history visits; `None` for bookmarks.
    pub visited_at: Option<i64>,
    /// Seconds since the Unix epoch (UTC) when a bookmark was first saved;
    /// `None` for history visits and for bookmarks saved before dates were
    /// kept.
    pub saved_at: Option<i64>,
}

/// Bookmarks, the history choice and recent visits, read from one snapshot.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SavedContent {
    pub bookmarks: Vec<SavedPage>,
    pub history_enabled: bool,
    /// Newest first.
    pub history: Vec<SavedPage>,
}

#[derive(uniffi::Object)]
pub struct ReaderState {
    database: PathBuf,
}

impl ReaderState {
    fn open(&self) -> Result<UserState, ReaderStateError> {
        UserState::open(&self.database).map_err(|_| ReaderStateError::Storage)
    }

    fn key(project: String, page_id: u64) -> Result<ArticleKey, ReaderStateError> {
        let key = ArticleKey { project, page_id };
        key.validate().map_err(|_| ReaderStateError::InvalidInput)?;
        Ok(key)
    }

    /// Without history, an unbookmarked page keeps no reading position.
    fn forget_position_unless_history(
        state: &UserState,
        generation: u64,
        key: &ArticleKey,
    ) -> Result<(), ReaderStateError> {
        if !state.history_enabled().map_err(store_error)? {
            state
                .remove_reading_position_for_generation(generation, key)
                .map_err(store_error)?;
        }
        Ok(())
    }

    fn check_revision(revision_id: u64) -> Result<(), ReaderStateError> {
        if revision_id == 0 || revision_id > i64::MAX as u64 {
            return Err(ReaderStateError::InvalidInput);
        }
        Ok(())
    }
}

/// The title of a saved page in the active pack, if it has it.
fn title_in(library: Option<&OfflineLibrary>, key: &ArticleKey) -> Option<String> {
    library?
        .open_page(key.project.clone(), key.page_id)
        .ok()
        .flatten()
        .map(|page| page.display_title)
}

#[uniffi::export]
impl ReaderState {
    /// Open (creating if needed) the user-state database in an absolute
    /// app-private directory; the same one `ChatHistory` uses.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, ReaderStateError> {
        let requested = PathBuf::from(&data_dir);
        if !requested.is_absolute() || data_dir.contains('\0') {
            return Err(ReaderStateError::InvalidInput);
        }
        std::fs::create_dir_all(&requested).map_err(|_| ReaderStateError::Storage)?;
        let root = std::fs::canonicalize(&requested).map_err(|_| ReaderStateError::Storage)?;
        let state = Self {
            database: root.join("chat-history.sqlite"),
        };
        state.open()?;
        Ok(Arc::new(state))
    }

    pub fn is_bookmarked(&self, project: String, page_id: u64) -> Result<bool, ReaderStateError> {
        let key = Self::key(project, page_id)?;
        Ok(self.open()?.bookmark(&key).map_err(store_error)?.is_some())
    }

    /// Add or remove the page's bookmark (at the shown revision); returns
    /// whether it is bookmarked now.
    pub fn toggle_bookmark(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
    ) -> Result<bool, ReaderStateError> {
        let key = Self::key(project, page_id)?;
        Self::check_revision(revision_id)?;
        let state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        let saved = if state.bookmark(&key).map_err(store_error)?.is_some() {
            if !state
                .remove_bookmark_for_generation(generation, &key)
                .map_err(store_error)?
            {
                return Err(ReaderStateError::Changed);
            }
            Self::forget_position_unless_history(&state, generation, &key)?;
            false
        } else {
            // An empty anchor is a page-level bookmark.
            if !state
                .set_bookmark_for_generation(generation, &key, revision_id, "")
                .map_err(store_error)?
            {
                return Err(ReaderStateError::Changed);
            }
            true
        };
        Ok(saved)
    }

    /// Remove a bookmark; returns whether there was one.
    pub fn remove_bookmark(&self, project: String, page_id: u64) -> Result<bool, ReaderStateError> {
        let key = Self::key(project, page_id)?;
        let state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        let removed = state
            .remove_bookmark_for_generation(generation, &key)
            .map_err(store_error)?;
        Self::forget_position_unless_history(&state, generation, &key)?;
        Ok(removed)
    }

    /// Where the page was left (millionths of its scrollable length), only
    /// for the exact revision it was saved for.
    pub fn position(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
    ) -> Result<Option<u32>, ReaderStateError> {
        let key = Self::key(project, page_id)?;
        Self::check_revision(revision_id)?;
        let saved = self
            .open()?
            .reading_position_for_revision(&key, revision_id)
            .map_err(store_error)?;
        Ok(saved
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value <= POSITION_SCALE))
    }

    /// Remember where the page was left, only while history is on or the
    /// page is bookmarked; returns whether it was saved.
    pub fn save_position(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
        position: u32,
    ) -> Result<bool, ReaderStateError> {
        let key = Self::key(project, page_id)?;
        Self::check_revision(revision_id)?;
        if position > POSITION_SCALE {
            return Err(ReaderStateError::InvalidInput);
        }
        let state = self.open()?;
        // Captured first: turning history off or clearing advances it, so
        // the guarded write below then changes nothing.
        let generation = state.personal_generation().map_err(store_error)?;
        let allowed = state.history_enabled().map_err(store_error)?
            || state.bookmark(&key).map_err(store_error)?.is_some();
        if !allowed {
            return Ok(false);
        }
        state
            .set_reading_position_for_generation(generation, &key, revision_id, u64::from(position))
            .map_err(store_error)
    }

    /// Record that the page was shown, only if history is on; returns
    /// whether a visit was saved.
    pub fn record_visit(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
    ) -> Result<bool, ReaderStateError> {
        let key = Self::key(project, page_id)?;
        Self::check_revision(revision_id)?;
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ReaderStateError::Storage)?
            .as_secs();
        let now = i64::try_from(seconds).map_err(|_| ReaderStateError::Storage)?;
        let state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        state
            .record_visit_for_generation(generation, &key, revision_id, now)
            .map_err(store_error)
    }

    /// Turn history recording on or off. Turning it off fences any visit
    /// still being written; saved visits stay until `clear_history`.
    pub fn set_history_enabled(&self, enabled: bool) -> Result<(), ReaderStateError> {
        let mut state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        if !state
            .set_history_enabled_for_generation(generation, enabled)
            .map_err(store_error)?
        {
            return Err(ReaderStateError::Changed);
        }
        Ok(())
    }

    /// Delete all visits and the reading positions of pages that are not
    /// bookmarked; returns how many visits there were. Bookmarks are kept.
    pub fn clear_history(&self) -> Result<u64, ReaderStateError> {
        let mut state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        let removed = state
            .clear_history_and_unbookmarked_positions_for_generation(generation)
            .map_err(store_error)?
            .ok_or(ReaderStateError::Changed)?;
        Ok(removed as u64)
    }

    /// Bookmarks, the history choice and recent visits, with titles from
    /// `library`'s active pack where it has the page.
    pub fn saved_content(
        &self,
        library: Option<Arc<OfflineLibrary>>,
    ) -> Result<SavedContent, ReaderStateError> {
        let state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        let (bookmarks, history_enabled, visits) = state
            .saved_panel_snapshot_for_generation(generation)
            .map_err(|error| match error {
                UserStateError::InvalidValue => ReaderStateError::Changed,
                _ => ReaderStateError::Storage,
            })?;
        let library = library.as_deref();
        Ok(SavedContent {
            bookmarks: bookmarks
                .into_iter()
                .take(MAX_BOOKMARKS)
                .map(|saved| SavedPage {
                    title: title_in(library, &saved.key),
                    project: saved.key.project,
                    page_id: saved.key.page_id,
                    revision_id: saved.revision_id,
                    visited_at: None,
                    saved_at: saved.saved_at,
                })
                .collect(),
            history_enabled,
            history: visits
                .into_iter()
                .map(|visit| SavedPage {
                    title: title_in(library, &visit.key),
                    project: visit.key.project,
                    page_id: visit.key.page_id,
                    revision_id: visit.revision_id,
                    visited_at: Some(visit.visited_at),
                    saved_at: None,
                })
                .collect(),
        })
    }
}
