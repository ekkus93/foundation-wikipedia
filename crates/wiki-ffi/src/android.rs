//! MOB-001 Android-callable API, exported through UniFFI.
//!
//! An `OfflineLibrary` owns one app data directory: it installs custom
//! `.wpack` files into inactive snapshots, activates one, and searches and
//! opens articles through the verified offline reader — the same shared Rust
//! code the desktop uses. Nothing here touches the network.
//!
//! All functions block. Kotlin must call them off the main thread (the
//! generated bindings are wrapped by `org.foundation.wikipedia.rust`, which
//! enforces that). This module contains no hand-written `unsafe`; UniFFI's
//! generated scaffolding carries its own scoped allowance (see Cargo.toml).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wiki_model::{Article, ArticleKey};
use wiki_store::activation::ActiveSnapshotStore;
use wiki_store::offline_reader::VerifiedOfflineCatalogReader;
use wiki_store::pack_install::{
    install_custom_pack, installed_shard_inventory, reclaim_abandoned_staging, PackInstallError,
    CATALOG_OBJECT, SEARCH_OBJECT,
};

const MAX_PACK_BYTES: u64 = 8 << 30;
const MAX_QUERY_BYTES: usize = 1024;
const MAX_RESULTS: u32 = 100;

/// Typed errors surfaced to Kotlin as exceptions.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum LibraryError {
    /// A path, query, identifier or limit was rejected before any work.
    InvalidInput,
    /// The pack claims official origin; signatures are not verifiable yet.
    UnsignedOfficialPack,
    /// The user has not accepted installing this custom pack.
    ConfirmationRequired,
    /// The pack or snapshot failed integrity or layout verification.
    VerificationFailed,
    AlreadyInstalled,
    /// No snapshot is active, or the requested one is not installed.
    NotInstalled,
    /// Packs with media cannot be installed yet.
    MediaNotSupported,
    /// Filesystem or database failure.
    Storage,
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for LibraryError {}

impl From<PackInstallError> for LibraryError {
    fn from(error: PackInstallError) -> Self {
        match error {
            PackInstallError::UnsignedOfficialPack => Self::UnsignedOfficialPack,
            PackInstallError::Declined => Self::ConfirmationRequired,
            PackInstallError::AlreadyInstalled => Self::AlreadyInstalled,
            PackInstallError::MediaNotSupported => Self::MediaNotSupported,
            PackInstallError::Io(_) | PackInstallError::UnsafeRoot => Self::Storage,
            _ => Self::VerificationFailed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InstalledPack {
    pub snapshot_id: String,
    pub project: String,
    /// Accessible trust text, e.g. "Custom — integrity verified".
    pub trust_label: String,
    pub articles: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchHit {
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArticlePage {
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    pub title: String,
    pub display_title: String,
    /// Canonical rendered article HTML from the installed snapshot.
    pub html: String,
}

impl From<&Article> for SearchHit {
    fn from(article: &Article) -> Self {
        Self {
            project: article.key.project.clone(),
            page_id: article.key.page_id,
            revision_id: article.revision.revision_id,
            title: article.title.clone(),
        }
    }
}

struct OpenSnapshot {
    id: String,
    reader: VerifiedOfflineCatalogReader,
}

/// One app-private offline library. Create a single instance per data
/// directory per process: it assumes exclusive ownership of that directory.
#[derive(uniffi::Object)]
pub struct OfflineLibrary {
    root: PathBuf,
    open: Mutex<Option<OpenSnapshot>>,
}

fn open_snapshot(root: &Path, id: &str) -> Result<VerifiedOfflineCatalogReader, LibraryError> {
    let snapshot = root.join("snapshots").join(id);
    if !snapshot.is_dir() {
        return Err(LibraryError::NotInstalled);
    }
    let inventory = installed_shard_inventory(&snapshot)?;
    VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &snapshot.join(CATALOG_OBJECT),
        &snapshot.join(SEARCH_OBJECT),
        &snapshot,
        &inventory,
    )
    .map_err(|_| LibraryError::VerificationFailed)
}

#[uniffi::export]
impl OfflineLibrary {
    /// Open (creating if needed) the library at an absolute app-private
    /// directory. Clears residue an interrupted install left in staging.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, LibraryError> {
        let root = PathBuf::from(&data_dir);
        if !root.is_absolute() || data_dir.contains('\0') {
            return Err(LibraryError::InvalidInput);
        }
        std::fs::create_dir_all(root.join("snapshots")).map_err(|_| LibraryError::Storage)?;
        reclaim_abandoned_staging(&root)?;
        ActiveSnapshotStore::new(&root)
            .reclaim_orphaned_selector_files()
            .map_err(|_| LibraryError::Storage)?;
        Ok(Arc::new(Self {
            root,
            open: Mutex::new(None),
        }))
    }

    /// Verify and install a custom `.wpack` file as an inactive snapshot.
    /// `accept_custom_pack` must reflect the user's explicit confirmation of
    /// an unsigned custom pack; official-origin packs are always refused.
    pub fn install_pack(
        &self,
        pack_path: String,
        accept_custom_pack: bool,
    ) -> Result<InstalledPack, LibraryError> {
        let path = PathBuf::from(&pack_path);
        if !path.is_absolute() {
            return Err(LibraryError::InvalidInput);
        }
        let mut file = File::open(&path).map_err(|_| LibraryError::InvalidInput)?;
        let installed = install_custom_pack(&self.root, &mut file, MAX_PACK_BYTES, |_| {
            accept_custom_pack
        })?;
        Ok(InstalledPack {
            snapshot_id: installed.snapshot_id,
            project: installed.manifest.project.clone(),
            trust_label: installed.trust.label().to_owned(),
            articles: installed.manifest.metadata.counts.articles,
        })
    }

    /// Make an installed snapshot the active one, after verified opening.
    pub fn activate(&self, snapshot_id: String) -> Result<(), LibraryError> {
        let reader = open_snapshot(&self.root, &snapshot_id)?;
        ActiveSnapshotStore::new(&self.root)
            .switch_to_prevalidated(&snapshot_id)
            .map_err(|_| LibraryError::Storage)?;
        *self.open.lock().map_err(|_| LibraryError::Storage)? = Some(OpenSnapshot {
            id: snapshot_id,
            reader,
        });
        Ok(())
    }

    pub fn active_snapshot(&self) -> Result<Option<String>, LibraryError> {
        ActiveSnapshotStore::new(&self.root)
            .current()
            .map_err(|_| LibraryError::Storage)
    }

    /// Full-text search of the active snapshot, best match first.
    pub fn search(&self, query: String, limit: u32) -> Result<Vec<SearchHit>, LibraryError> {
        if query.trim().is_empty() || query.len() > MAX_QUERY_BYTES || limit == 0 {
            return Err(LibraryError::InvalidInput);
        }
        self.with_active(|reader| {
            reader
                .reader()
                .search_articles(&query, limit.min(MAX_RESULTS) as usize)
                .map(|articles| articles.iter().map(SearchHit::from).collect())
                .map_err(|_| LibraryError::VerificationFailed)
        })
    }

    /// Open one article of the active snapshot; `None` if not installed.
    pub fn open_page(
        &self,
        project: String,
        page_id: u64,
    ) -> Result<Option<ArticlePage>, LibraryError> {
        let key = ArticleKey { project, page_id };
        key.validate().map_err(|_| LibraryError::InvalidInput)?;
        self.with_active(|reader| {
            let article = reader
                .reader()
                .open_page(&key)
                .map_err(|_| LibraryError::VerificationFailed)?;
            Ok(article.map(|article| ArticlePage {
                project: article.key.project,
                page_id: article.key.page_id,
                revision_id: article.revision.revision_id,
                title: article.title,
                display_title: article.display_title,
                html: article.rendered_html,
            }))
        })
    }
}

impl OfflineLibrary {
    /// Run against the active snapshot, (re)opening it if the selector moved.
    fn with_active<T>(
        &self,
        run: impl FnOnce(&VerifiedOfflineCatalogReader) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        let active = self.active_snapshot()?.ok_or(LibraryError::NotInstalled)?;
        let mut open = self.open.lock().map_err(|_| LibraryError::Storage)?;
        if open.as_ref().is_none_or(|snapshot| snapshot.id != active) {
            *open = Some(OpenSnapshot {
                reader: open_snapshot(&self.root, &active)?,
                id: active,
            });
        }
        run(&open.as_ref().expect("snapshot opened above").reader)
    }
}
