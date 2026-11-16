//! DUI-001: installed offline packs on the desktop.
//!
//! The desktop uses the same shared `OfflineLibrary` as Android (verified
//! install, activation, offline search and reading), in a private folder of
//! the app's data directory. Pack HTML is untrusted: it is sanitized with
//! the pack sanitizer, which removes image sources (no offline media yet)
//! and turns article links into `https://foundation.invalid/wiki/<Title>`
//! for in-app navigation within the active pack. Nothing here uses the
//! network.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use wiki_ffi::android::{
    pack_trust_description, InstallReviewer, LibraryError, OfflineLibrary, PackReview, PackSource,
};

/// Packs larger than this are refused by the import dialog (the shared
/// library's own limit).
const MAX_PACK_BYTES: u64 = 8 << 30;

/// The app's one offline library, opened on first use.
#[derive(Default)]
pub struct OfflineLibraryState(Mutex<Option<Arc<OfflineLibrary>>>);

fn library(app: &tauri::AppHandle) -> Result<Arc<OfflineLibrary>, String> {
    let state = app.state::<OfflineLibraryState>();
    let mut slot = state.0.lock().map_err(|_| "Offline library unavailable")?;
    if let Some(library) = slot.as_ref() {
        return Ok(library.clone());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Cannot find the app data directory")?
        .join("offline-library");
    std::fs::create_dir_all(&dir).map_err(|_| "Cannot create the offline library folder")?;
    let library = OfflineLibrary::new(dir.to_string_lossy().into_owned()).map_err(message)?;
    *slot = Some(library.clone());
    Ok(library)
}

/// Plain-language text for a failed pack operation.
fn message(error: LibraryError) -> String {
    match error {
        LibraryError::VerificationFailed => {
            "This pack failed verification: it is damaged or was modified. Nothing was installed."
        }
        LibraryError::UnsignedOfficialPack => {
            "This pack claims to be official but came without a verifiable publisher signature, so it was not installed."
        }
        LibraryError::ConfirmationRequired => "Import cancelled. Nothing was installed.",
        LibraryError::AlreadyInstalled => "This pack is already installed.",
        LibraryError::MediaNotSupported => {
            "This pack includes images or media, which cannot be installed yet."
        }
        LibraryError::SnapshotActive => "This pack is in use. Activate another pack before removing it.",
        LibraryError::NotInstalled => "No offline pack is active.",
        LibraryError::Storage => "Storage error. Check free space and try again.",
        _ => "The offline pack request was not valid.",
    }
    .to_owned()
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "Offline pack worker stopped".to_owned())?
}

/// Ask the user for a `.wpack` file. `None` if they cancelled.
#[tauri::command]
pub async fn pick_offline_pack(app: tauri::AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
        Ok(app
            .dialog()
            .file()
            .set_title("Import a Wikipedia pack")
            .add_filter("Wikipedia pack", &["wpack"])
            .blocking_pick_file()
            .and_then(|file| file.into_path().ok())
            .map(|path| path.to_string_lossy().into_owned()))
    })
    .await
}

#[derive(Serialize)]
pub struct PackReviewDto {
    snapshot_id: String,
    project: String,
    trust_label: String,
    trust_description: String,
    articles: u64,
    declared_bytes: u64,
}

struct FileSource(Mutex<std::fs::File>);

impl PackSource for FileSource {
    fn read(&self, max_bytes: u32) -> Result<Vec<u8>, LibraryError> {
        use std::io::Read;
        let mut buffer = vec![0; max_bytes as usize];
        let read = self
            .0
            .lock()
            .map_err(|_| LibraryError::SourceFailed)?
            .read(&mut buffer)
            .map_err(|_| LibraryError::SourceFailed)?;
        buffer.truncate(read);
        Ok(buffer)
    }
}

/// Captures the verified review and declines, so nothing is installed.
struct ReviewOnly(Mutex<Option<PackReview>>);

impl InstallReviewer for ReviewOnly {
    fn approve(&self, review: PackReview) -> bool {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(review);
        }
        false
    }
}

fn pack_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("Choose a .wpack file.".to_owned());
    }
    Ok(path)
}

/// Fully verify a pack and describe it for the user's confirmation,
/// without installing anything.
#[tauri::command]
pub async fn review_offline_pack(
    app: tauri::AppHandle,
    path: String,
) -> Result<PackReviewDto, String> {
    blocking(move || {
        let path = pack_path(&path)?;
        let library = library(&app)?;
        let file =
            std::fs::File::open(&path).map_err(|_| "The selected file could not be read.")?;
        let reviewer = Arc::new(ReviewOnly(Mutex::new(None)));
        let result = library.install_pack_stream(
            Arc::new(FileSource(Mutex::new(file))),
            reviewer.clone(),
            MAX_PACK_BYTES,
        );
        let review = reviewer.0.lock().ok().and_then(|mut slot| slot.take());
        match (result, review) {
            (Err(LibraryError::ConfirmationRequired), Some(review)) => Ok(PackReviewDto {
                trust_description: pack_trust_description(review.trust),
                snapshot_id: review.snapshot_id,
                project: review.project,
                trust_label: review.trust_label,
                articles: review.articles,
                declared_bytes: review.declared_bytes,
            }),
            (Err(error), _) => Err(message(error)),
            (Ok(_), _) => Err("The pack was installed without review.".to_owned()),
        }
    })
    .await
}

/// Install a reviewed custom pack after the user confirmed it (verified
/// again; it never replaces an installed pack).
#[tauri::command]
pub async fn install_offline_pack(app: tauri::AppHandle, path: String) -> Result<String, String> {
    blocking(move || {
        let path = pack_path(&path)?;
        let installed = library(&app)?
            .install_pack(path.to_string_lossy().into_owned(), true)
            .map_err(message)?;
        Ok(installed.snapshot_id)
    })
    .await
}

#[derive(Serialize)]
pub struct InstalledPackDto {
    snapshot_id: String,
    project: String,
    trust_label: String,
    trust_description: String,
    articles: u64,
    active: bool,
    damaged: bool,
}

#[tauri::command]
pub async fn list_offline_packs(app: tauri::AppHandle) -> Result<Vec<InstalledPackDto>, String> {
    blocking(move || {
        Ok(library(&app)?
            .list_packs()
            .map_err(message)?
            .into_iter()
            .map(|pack| InstalledPackDto {
                trust_description: pack_trust_description(pack.trust),
                snapshot_id: pack.snapshot_id,
                project: pack.project,
                trust_label: pack.trust_label,
                articles: pack.articles,
                active: pack.active,
                damaged: pack.damaged,
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn activate_offline_pack(
    app: tauri::AppHandle,
    snapshot_id: String,
) -> Result<(), String> {
    blocking(move || library(&app)?.activate(snapshot_id).map_err(message)).await
}

#[tauri::command]
pub async fn remove_offline_pack(app: tauri::AppHandle, snapshot_id: String) -> Result<(), String> {
    blocking(move || library(&app)?.remove_pack(snapshot_id).map_err(message)).await
}

/// Search the active pack (no network). `(project, page ID, title)`.
#[tauri::command]
pub async fn search_offline_packs(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<(String, u64, String)>, String> {
    blocking(move || {
        Ok(library(&app)?
            .search(query, 20)
            .map_err(message)?
            .into_iter()
            .map(|hit| (hit.project, hit.page_id, hit.title))
            .collect())
    })
    .await
}

/// `(page ID, revision ID, title, sanitized body HTML)` of an installed
/// article, or `None` if the active pack does not have it.
pub type OfflineArticle = Option<(u64, u64, String, String)>;

fn sanitized(page: wiki_ffi::android::ArticlePage) -> (u64, u64, String, String) {
    (
        page.page_id,
        page.revision_id,
        page.display_title,
        wiki_source::reader_document::sanitize_article_html(&page.html),
    )
}

#[tauri::command]
pub async fn open_offline_page(
    app: tauri::AppHandle,
    project: String,
    page_id: u64,
) -> Result<OfflineArticle, String> {
    blocking(move || {
        Ok(library(&app)?
            .open_page(project, page_id)
            .map_err(message)?
            .map(sanitized))
    })
    .await
}

/// Resolve an in-pack article link (title or installed redirect).
#[tauri::command]
pub async fn open_offline_title(
    app: tauri::AppHandle,
    project: String,
    title: String,
) -> Result<OfflineArticle, String> {
    blocking(move || {
        let library = library(&app)?;
        let Some(page) = library
            .open_reader_title(project.clone(), title, false)
            .map_err(message)?
        else {
            return Ok(None);
        };
        Ok(library
            .open_page(project, page.page_id)
            .map_err(message)?
            .map(sanitized))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_absolute_files_are_accepted_as_packs() {
        assert!(pack_path("relative/pack.wpack").is_err());
        assert!(pack_path("/definitely/not/here.wpack").is_err());
        let dir = std::env::temp_dir();
        assert!(pack_path(&dir.to_string_lossy()).is_err(), "a directory");
        let file = dir.join(format!("offline-packs-{}.wpack", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(pack_path(&file.to_string_lossy()).unwrap(), file);
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn failures_are_explained_without_claiming_trust() {
        let failed = message(LibraryError::VerificationFailed);
        assert!(failed.contains("Nothing was installed"));
        let official = message(LibraryError::UnsignedOfficialPack);
        assert!(official.contains("not installed"));
        assert!(!official.to_lowercase().contains("verified publisher"));
        assert!(message(LibraryError::SnapshotActive).contains("Activate another pack"));
    }
}
