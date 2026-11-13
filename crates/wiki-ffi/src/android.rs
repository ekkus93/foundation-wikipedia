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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wiki_model::{Article, ArticleKey};
use wiki_store::activation::ActiveSnapshotStore;
use wiki_store::offline_reader::VerifiedOfflineCatalogReader;
use wiki_store::pack_install::{
    install_custom_pack, install_custom_pack_stream, installed_shard_inventory,
    list_installed_snapshots, reclaim_abandoned_staging, remove_installed_snapshot,
    PackInstallError, CATALOG_OBJECT, SEARCH_OBJECT,
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
    /// The snapshot is active; activate another before removing it.
    SnapshotActive,
    /// The app-side pack source failed while being read.
    SourceFailed,
}

impl From<uniffi::UnexpectedUniFFICallbackError> for LibraryError {
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::SourceFailed
    }
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
            PackInstallError::SnapshotActive => Self::SnapshotActive,
            PackInstallError::NotInstalled => Self::NotInstalled,
            PackInstallError::InvalidSnapshotId => Self::InvalidInput,
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

/// What the user reviews before an import is published.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PackReview {
    pub snapshot_id: String,
    pub project: String,
    /// Always "Custom — integrity verified" for an installable pack; never
    /// a publisher-verified label (official packs are refused).
    pub trust_label: String,
    pub articles: u64,
    pub declared_bytes: u64,
}

/// An installed snapshot as listed for the pack manager.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InstalledSnapshot {
    pub snapshot_id: String,
    pub project: String,
    pub trust_label: String,
    pub articles: u64,
    pub active: bool,
    /// Its installer manifest is unreadable; it can only be removed.
    pub damaged: bool,
}

/// App-supplied pack bytes (e.g. an Android document-picker stream).
/// `read` returns at most `max_bytes`; an empty result means end of stream.
#[uniffi::export(with_foreign)]
pub trait PackSource: Send + Sync {
    fn read(&self, max_bytes: u32) -> Result<Vec<u8>, LibraryError>;
}

/// Asked once, after full verification and before publication. Returning
/// false discards the staged pack. Called on the installing thread.
#[uniffi::export(with_foreign)]
pub trait InstallReviewer: Send + Sync {
    fn approve(&self, review: PackReview) -> bool;
}

const SOURCE_CHUNK: usize = 64 * 1024;

/// Adapts a foreign `PackSource` to `std::io::Read`.
struct SourceReader(Arc<dyn PackSource>);

impl Read for SourceReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let want = buf.len().min(SOURCE_CHUNK);
        let chunk = self
            .0
            .read(want as u32)
            .map_err(|_| std::io::Error::other("pack source failed"))?;
        if chunk.len() > want {
            return Err(std::io::Error::other("pack source overran its request"));
        }
        buf[..chunk.len()].copy_from_slice(&chunk);
        Ok(chunk.len())
    }
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
        let requested = PathBuf::from(&data_dir);
        if !requested.is_absolute() || data_dir.contains('\0') {
            return Err(LibraryError::InvalidInput);
        }
        std::fs::create_dir_all(requested.join("snapshots")).map_err(|_| LibraryError::Storage)?;
        // Resolve the app-supplied path once. Android's app directories sit
        // under /data/user/0, a system symlink to /data/data; the store
        // rejects symlinked ancestors, so it must be given the real path.
        let root = std::fs::canonicalize(&requested).map_err(|_| LibraryError::Storage)?;
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

    /// Verify and install a pack streamed by the app (no seeking needed),
    /// spooling at most `max_bytes`. `reviewer` sees the verified pack and
    /// must approve before it is published as an inactive snapshot.
    pub fn install_pack_stream(
        &self,
        source: Arc<dyn PackSource>,
        reviewer: Arc<dyn InstallReviewer>,
        max_bytes: u64,
    ) -> Result<InstalledPack, LibraryError> {
        if max_bytes == 0 || max_bytes > MAX_PACK_BYTES {
            return Err(LibraryError::InvalidInput);
        }
        let mut reader = SourceReader(source);
        let installed = install_custom_pack_stream(&self.root, &mut reader, max_bytes, |review| {
            reviewer.approve(PackReview {
                snapshot_id: review.manifest.pack_id.clone(),
                project: review.manifest.project.clone(),
                trust_label: review.trust.label().to_owned(),
                articles: review.manifest.metadata.counts.articles,
                declared_bytes: review.declared_bytes,
            })
        })?;
        Ok(InstalledPack {
            snapshot_id: installed.snapshot_id,
            project: installed.manifest.project.clone(),
            trust_label: installed.trust.label().to_owned(),
            articles: installed.manifest.metadata.counts.articles,
        })
    }

    /// Installed snapshots in ID order, with the active one marked.
    pub fn list_packs(&self) -> Result<Vec<InstalledSnapshot>, LibraryError> {
        let active = self.active_snapshot()?;
        Ok(list_installed_snapshots(&self.root)?
            .into_iter()
            .map(|info| InstalledSnapshot {
                active: active.as_deref() == Some(info.snapshot_id.as_str()),
                snapshot_id: info.snapshot_id,
                project: info.project,
                trust_label: info.trust_label,
                articles: info.articles,
                damaged: info.damaged,
            })
            .collect())
    }

    /// Remove an inactive installed snapshot (refused while active).
    pub fn remove_pack(&self, snapshot_id: String) -> Result<(), LibraryError> {
        remove_installed_snapshot(&self.root, &snapshot_id)?;
        let mut open = self.open.lock().map_err(|_| LibraryError::Storage)?;
        if open
            .as_ref()
            .is_some_and(|snapshot| snapshot.id == snapshot_id)
        {
            *open = None;
        }
        Ok(())
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

// ---------------------------------------------------------------------------
// AI-003 hosted models on Android.
//
// The app owns API keys (Android Keystore-encrypted storage) and passes one
// in per explicit request; nothing here stores, logs or returns a key. TLS
// uses the shared rustls transport with Android's certificate verifier,
// which the app must initialise first (`RustTls.init`, see android_tls.rs).

const MAX_MODEL_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HostedAiProvider {
    Anthropic,
    OpenAi,
}

impl From<HostedAiProvider> for wiki_ai::hosted_codec::HostedProvider {
    fn from(provider: HostedAiProvider) -> Self {
        match provider {
            HostedAiProvider::Anthropic => Self::Anthropic,
            HostedAiProvider::OpenAi => Self::OpenAi,
        }
    }
}

/// Hosted-model failures. None carries key material or response text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum HostedAiError {
    /// The key is empty, too long, or has whitespace or control characters.
    InvalidKey,
    /// The model name or request was rejected before any network traffic.
    InvalidRequest,
    /// The OS certificate verifier has not been initialised on this device.
    NotReady,
    /// The provider refused the request (for example an invalid key).
    Rejected,
    Timeout,
    /// Connection, TLS or certificate failure.
    Network,
    InvalidResponse,
}

impl std::fmt::Display for HostedAiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for HostedAiError {}

impl From<wiki_ai::ProviderError> for HostedAiError {
    fn from(error: wiki_ai::ProviderError) -> Self {
        use wiki_ai::ProviderError as E;
        match error {
            E::InvalidRequest | E::DestinationNotAllowed => Self::InvalidRequest,
            E::Unsupported => Self::NotReady,
            E::Rejected => Self::Rejected,
            E::Timeout | E::Cancelled => Self::Timeout,
            E::Transport => Self::Network,
            E::InvalidResponse => Self::InvalidResponse,
        }
    }
}

/// Reply to an explicit connection check.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HostedAiCheck {
    pub reply: String,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

/// Whether hosted HTTPS can verify certificates (Android: after `RustTls.init`).
#[uniffi::export]
pub fn hosted_ai_ready() -> bool {
    wiki_ai::hosted_http::platform_verifier_ready()
}

/// Validate a key's shape before the app stores it. No network traffic.
#[uniffi::export]
pub fn hosted_ai_key_is_valid(api_key: String) -> bool {
    wiki_ai::hosted_codec::ApiKey::new(api_key).is_ok()
}

/// Send one tiny prompt to the user's hosted model with `api_key`. This IS
/// cloud traffic: call it only from an explicit user action that says so.
#[uniffi::export]
pub fn check_hosted_ai_connection(
    provider: HostedAiProvider,
    api_key: String,
    model: String,
) -> Result<HostedAiCheck, HostedAiError> {
    validate_model(&model)?;
    let key = wiki_ai::hosted_codec::ApiKey::new(api_key).map_err(|_| HostedAiError::InvalidKey)?;
    let hosted = wiki_ai::hosted_http::HostedChatProvider::new(provider.into(), key)?;
    run_hosted_check(&hosted, model)
}

fn validate_model(model: &str) -> Result<(), HostedAiError> {
    if model.is_empty()
        || model.len() > MAX_MODEL_BYTES
        || !model.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(HostedAiError::InvalidRequest);
    }
    Ok(())
}

/// The connection check against any provider (host tests use a fake one).
pub fn run_hosted_check(
    provider: &dyn wiki_ai::LlmProvider,
    model: String,
) -> Result<HostedAiCheck, HostedAiError> {
    use wiki_ai::{GenerateRequest, Message, MessageRole, OutboundPolicy, StreamEvent};
    validate_model(&model)?;
    let request = GenerateRequest {
        model,
        messages: vec![Message {
            role: MessageRole::User,
            content: "Reply with the single word OK.".into(),
        }],
        max_output_tokens: 16,
    };
    let mut check = HostedAiCheck {
        reply: String::new(),
        input_tokens: None,
        output_tokens: None,
    };
    wiki_ai::stream_selected(
        provider,
        &request,
        &wiki_ai::CancellationToken::default(),
        OutboundPolicy::AllowCloud,
        &mut |event| {
            match event {
                StreamEvent::TextDelta(text) => check.reply.push_str(&text),
                StreamEvent::Completed {
                    input_tokens,
                    output_tokens,
                } => {
                    check.input_tokens = input_tokens;
                    check.output_tokens = output_tokens;
                }
            }
            Ok(())
        },
    )?;
    Ok(check)
}
