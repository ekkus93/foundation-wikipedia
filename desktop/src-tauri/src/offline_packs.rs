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

pub(crate) fn library(app: &tauri::AppHandle) -> Result<Arc<OfflineLibrary>, String> {
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
        LibraryError::MediaInvalid => {
            "This pack's images are missing or damaged, so it was not installed."
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
    license_notice: String,
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

/// The pack the user last reviewed: its path and manifest identity.
#[derive(Default)]
pub struct ReviewedPack(pub Mutex<Option<(PathBuf, String)>>);

/// Approves only the pack whose identity the user reviewed.
struct SamePack(String);

impl InstallReviewer for SamePack {
    fn approve(&self, review: PackReview) -> bool {
        !self.0.is_empty() && review.identity_sha256 == self.0
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
            (Err(LibraryError::ConfirmationRequired), Some(review)) => {
                // Remember exactly which pack the user is shown, so install
                // can refuse a different file at the same path.
                if let Ok(mut reviewed) = app.state::<ReviewedPack>().0.lock() {
                    *reviewed = Some((path.clone(), review.identity_sha256.clone()));
                }
                Ok(PackReviewDto {
                    trust_description: pack_trust_description(review.trust),
                    snapshot_id: review.snapshot_id,
                    project: review.project,
                    trust_label: review.trust_label,
                    articles: review.articles,
                    declared_bytes: review.declared_bytes,
                    license_notice: review.license_notice,
                })
            }
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
        let reviewed = app
            .state::<ReviewedPack>()
            .0
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let Some((reviewed_path, identity)) = reviewed.filter(|(p, _)| *p == path) else {
            return Err("Review this pack before installing it.".to_owned());
        };
        let file = std::fs::File::open(&reviewed_path)
            .map_err(|_| "The selected file could not be read.")?;
        let installed = library(&app)?
            .install_pack_stream(
                Arc::new(FileSource(Mutex::new(file))),
                Arc::new(SamePack(identity)),
                MAX_PACK_BYTES,
            )
            .map_err(|error| match error {
                LibraryError::ConfirmationRequired => {
                    "The file changed after it was reviewed. Nothing was installed; review it again."
                        .to_owned()
                }
                other => message(other),
            })?;
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

/// Where the article frame loads pack images: the app's `packimage`
/// scheme, which Tauri exposes as `http://packimage.localhost/` on Windows.
pub const PACK_IMAGE_BASE: &str = if cfg!(windows) {
    "http://packimage.localhost/"
} else {
    "packimage://localhost/"
};

fn sanitized(page: wiki_ffi::android::ArticlePage) -> (u64, u64, String, String) {
    let images = wiki_source::reader_document::PackImageSources {
        base: PACK_IMAGE_BASE.to_owned(),
        by_file: page.images,
        stills: page.still_images.into_iter().collect(),
    };
    (
        page.page_id,
        page.revision_id,
        page.display_title,
        with_attribution(
            wiki_source::reader_document::sanitize_pack_article_html(&page.html, &images),
            &page.attribution_html,
        ),
    )
}

/// The page body followed by its attribution section (SEC-002).
fn with_attribution(body: String, attribution: &str) -> String {
    format!("{body}{attribution}")
}

/// Serve `packimage://localhost/<sha256>` from the active pack, re-hashed;
/// anything else is 404. Only `GET` of a bare digest path is answered.
pub fn pack_image_response(
    app: &tauri::AppHandle,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .expect("static response")
    };
    if request.method() != tauri::http::Method::GET {
        return not_found();
    }
    let digest = request.uri().path().trim_start_matches('/');
    let Ok(library) = library(app) else {
        return not_found();
    };
    match library.read_image(digest.to_owned()) {
        Ok(Some(image)) => tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", image.mime)
            .header("X-Content-Type-Options", "nosniff")
            .header(
                "Content-Security-Policy",
                wiki_store::pack_media::PACK_IMAGE_CSP,
            )
            .header("Cache-Control", "no-store")
            .body(image.bytes)
            .unwrap_or_else(|_| not_found()),
        _ => not_found(),
    }
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
    fn the_desktop_page_ends_with_the_attribution_section() {
        let page = wiki_ffi::android::ArticlePage {
            project: "enwiki".into(),
            page_id: 1,
            revision_id: 2,
            title: "T".into(),
            display_title: "T".into(),
            html: "<p>Body</p>".into(),
            images: Default::default(),
            still_images: vec![],
            attribution_html: "<section class=\"foundation-attribution\">credits</section>".into(),
        };
        let (_, _, _, html) = sanitized(page);
        assert!(html.starts_with("<p>Body</p>"), "{html}");
        assert!(
            html.ends_with("<section class=\"foundation-attribution\">credits</section>"),
            "{html}"
        );
    }

    #[test]
    fn tree_sizes_count_files_without_following_links() {
        let dir = std::env::temp_dir().join(format!("tree-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/one"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("a/b/two"), [0u8; 5]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/usr", dir.join("link")).unwrap();
        let mut budget = 100;
        assert_eq!(tree_bytes(&dir, &mut budget), 15);
        let mut tiny = 1;
        assert!(tree_bytes(&dir, &mut tiny) <= 15);
        assert_eq!(tree_bytes(&dir.join("missing"), &mut budget), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

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

/// Bytes used on disk, by area (STATE-002 Storage settings).
#[derive(Serialize)]
pub struct StorageUsageDto {
    /// `(pack ID, bytes)` of each installed snapshot.
    packs: Vec<(String, u64)>,
    /// Partial downloads kept for resuming.
    downloads: u64,
    /// Interrupted-install residue (cleared automatically).
    staging: u64,
    /// The private reading-state database (bookmarks, history, chats, settings).
    personal_data: u64,
}

/// Sum regular-file sizes under `path`, never following symbolic links and
/// visiting at most `budget` entries.
fn tree_bytes(path: &std::path::Path, budget: &mut usize) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        total += tree_bytes(&entry.path(), budget);
    }
    total
}

#[tauri::command]
pub async fn offline_storage_usage(app: tauri::AppHandle) -> Result<StorageUsageDto, String> {
    blocking(move || {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|_| "Cannot find the app data directory")?;
        let library = base.join("offline-library");
        let mut budget = 200_000;
        let mut packs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(library.join("snapshots")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                packs.push((name, tree_bytes(&entry.path(), &mut budget)));
            }
        }
        packs.sort();
        Ok(StorageUsageDto {
            packs,
            downloads: tree_bytes(&library.join("downloads"), &mut budget),
            staging: tree_bytes(&library.join("staging"), &mut budget),
            personal_data: tree_bytes(&base.join("reader-state"), &mut budget),
        })
    })
    .await
}

/// Where the app keeps its data, for the Advanced settings page.
#[tauri::command]
pub fn app_data_location(app: tauri::AppHandle) -> Result<String, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| "Cannot find the app data directory".to_owned())
}
