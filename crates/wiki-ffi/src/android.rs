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

// ---------------------------------------------------------------------------
// AI-003 article chat with a hosted model, grounded in an installed pack.
//
// `prepare_hosted_chat` builds the exact passages, model and host that would
// be sent, without any network I/O, for the app to disclose. `send` contacts
// the provider only with the user's consent to THAT preview; the reply is
// shown only after its citations are validated against the same verified
// article revision. With a `ChatHistory`, a supported answer is saved to its
// exact-revision thread with the cited evidence, and a follow-up question
// resends that thread's recent turns (disclosed in the preview).

const CHAT_PASSAGE_LIMIT: usize = 6;
const CHAT_CONTEXT_WORDS: usize = 900;
const CHAT_MAX_OUTPUT_TOKENS: u32 = 1024;
const MAX_CHAT_QUESTION_BYTES: usize = 2048;

/// Article-chat failures. None carries key material, passages or reply text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum ChatError {
    /// The question, model or article identity was rejected before any work.
    InvalidInput,
    InvalidKey,
    /// No pack is active, or the article is not in it.
    NotInstalled,
    /// The installed article failed verification.
    VerificationFailed,
    /// The article has no passage relevant to the question; nothing was sent.
    NoEvidence,
    /// The user did not consent to sending the disclosed passages.
    ConsentRequired,
    /// The preview no longer matches the article or selection; prepare again.
    PreviewChanged,
    /// This preview was already sent; prepare a new one for another request.
    AlreadySent,
    Cancelled,
    /// The OS certificate verifier has not been initialised (`RustTls.init`).
    NotReady,
    /// The provider refused the request (for example an invalid key or model).
    Rejected,
    Timeout,
    Network,
    InvalidResponse,
    /// The reply's citations did not match the evidence, so it is not shown.
    Unverifiable,
    /// The selected text is not within exactly one passage of the article.
    SelectionNotFound,
    /// Chat history could not be read or written.
    Storage,
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ChatError {}

impl From<wiki_ai::ProviderError> for ChatError {
    fn from(error: wiki_ai::ProviderError) -> Self {
        use wiki_ai::ProviderError as E;
        match error {
            E::Cancelled => Self::Cancelled,
            E::InvalidRequest | E::DestinationNotAllowed => Self::InvalidInput,
            other => match HostedAiError::from(other) {
                HostedAiError::NotReady => Self::NotReady,
                HostedAiError::Rejected => Self::Rejected,
                HostedAiError::Timeout => Self::Timeout,
                HostedAiError::Network => Self::Network,
                HostedAiError::InvalidResponse => Self::InvalidResponse,
                HostedAiError::InvalidKey | HostedAiError::InvalidRequest => Self::InvalidInput,
            },
        }
    }
}

impl From<wiki_ai::article_destination::ArticleInferenceError> for ChatError {
    fn from(error: wiki_ai::article_destination::ArticleInferenceError) -> Self {
        use wiki_ai::article_destination::ArticleInferenceError as E;
        use wiki_ai::rag::RagError as R;
        match error {
            E::PassageConsentRequired => Self::ConsentRequired,
            E::ModelMismatch | E::PreviewMismatch => Self::PreviewChanged,
            E::Rag(R::Provider(error)) => error.into(),
            E::Rag(R::InvalidArticle(_)) => Self::VerificationFailed,
            E::Rag(
                R::InvalidQuestion
                | R::InvalidSelection
                | R::SelectionTooLarge
                | R::InvalidPriorTurn
                | R::ContextTooLarge,
            ) => Self::InvalidInput,
            E::Rag(R::IncompleteStream | R::ResponseTooLarge) => Self::InvalidResponse,
            E::Rag(R::Citation(_) | R::MalformedCitation) | E::Display(_) => Self::Unverifiable,
        }
    }
}

/// What to ask, of which article of the active pack, and where.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HostedChatRequest {
    pub project: String,
    pub page_id: u64,
    pub question: String,
    pub provider: HostedAiProvider,
    pub model: String,
    /// Continue this saved thread (needs chat history); `None` starts one.
    pub thread_id: Option<i64>,
    /// Text the user selected in the reader. It must lie within one
    /// passage of the article, which is then always sent.
    pub selection: Option<String>,
}

// ---------------------------------------------------------------------------
// MOB-003 local or LAN model servers (for example Ollama on a computer on the
// same network, or `http://10.0.2.2:11434` from an Android emulator). Only
// IP literals or `localhost` are accepted, never DNS names or public hosts.

/// The protocol of a local model server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LocalAiProtocol {
    Ollama,
    OpenAiCompatible,
}

impl From<LocalAiProtocol> for wiki_ai::local_selection::LocalProtocol {
    fn from(protocol: LocalAiProtocol) -> Self {
        match protocol {
            LocalAiProtocol::Ollama => Self::Ollama,
            LocalAiProtocol::OpenAiCompatible => Self::OpenAiCompatible,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Error)]
pub enum LocalAiError {
    /// Not an `http://` URL with a private-network or loopback IP literal.
    InvalidEndpoint,
    /// Nothing answered at the address, or the connection failed.
    Unreachable,
    Timeout,
    /// The server answered, but not like the chosen protocol.
    InvalidResponse,
    /// The server lists no models.
    NoModels,
    Cancelled,
}

impl std::fmt::Display for LocalAiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for LocalAiError {}

impl From<wiki_ai::local_selection::SelectionError> for LocalAiError {
    fn from(error: wiki_ai::local_selection::SelectionError) -> Self {
        use wiki_ai::local_selection::SelectionError as S;
        use wiki_ai::ProviderError as P;
        match error {
            S::InvalidEndpoint(_) | S::UnknownModel => Self::InvalidEndpoint,
            S::Provider(P::Cancelled) => Self::Cancelled,
            S::Provider(P::Timeout) => Self::Timeout,
            S::Provider(P::Transport) => Self::Unreachable,
            S::Provider(P::InvalidRequest | P::DestinationNotAllowed | P::Unsupported) => {
                Self::InvalidEndpoint
            }
            S::Provider(P::Rejected | P::InvalidResponse) => Self::InvalidResponse,
        }
    }
}

/// Where a local server is, as disclosed to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ChatLocality {
    /// A hosted provider on the internet.
    Cloud,
    /// Another device on the local network (or the emulator's host).
    LocalNetwork,
    /// A server on this device.
    ThisDevice,
}

impl From<wiki_ai::Locality> for ChatLocality {
    fn from(locality: wiki_ai::Locality) -> Self {
        match locality {
            wiki_ai::Locality::Cloud => Self::Cloud,
            wiki_ai::Locality::LocalNetwork => Self::LocalNetwork,
            wiki_ai::Locality::Localhost | wiki_ai::Locality::OnDevice => Self::ThisDevice,
        }
    }
}

/// A local model server whose model list was just read (`discover_local_ai`).
#[derive(uniffi::Object)]
pub struct LocalAiServer {
    inventory: wiki_ai::local_selection::LocalModelInventory,
}

#[uniffi::export]
impl LocalAiServer {
    pub fn endpoint(&self) -> String {
        self.inventory.endpoint().to_owned()
    }

    pub fn locality(&self) -> ChatLocality {
        self.inventory.locality().into()
    }

    /// The model IDs the server offers; only these can be selected.
    pub fn models(&self) -> Vec<String> {
        self.inventory
            .models()
            .iter()
            .map(|model| model.id.clone())
            .collect()
    }
}

/// Read the model list of the local server at `endpoint`. This is a single
/// GET of the model list: no question or article text is sent.
#[uniffi::export]
pub fn discover_local_ai(
    protocol: LocalAiProtocol,
    endpoint: String,
) -> Result<Arc<LocalAiServer>, LocalAiError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if !endpoint.starts_with("http://") {
        return Err(LocalAiError::InvalidEndpoint);
    }
    let inventory = wiki_ai::local_selection::discover_local_models(protocol.into(), endpoint)?;
    if inventory.models().is_empty() {
        return Err(LocalAiError::NoModels);
    }
    Ok(Arc::new(LocalAiServer { inventory }))
}

/// What to ask a local server about an article of the active pack.
#[derive(Clone, uniffi::Record)]
pub struct LocalChatRequest {
    pub project: String,
    pub page_id: u64,
    pub question: String,
    pub server: Arc<LocalAiServer>,
    /// One of `server.models()`.
    pub model: String,
    pub thread_id: Option<i64>,
    pub selection: Option<String>,
}

// ---------------------------------------------------------------------------
// Progress of a sent request, for a progress bar and a time estimate. Model
// output is still buffered until its citations are validated; only counts
// and timings are reported, never text.

/// A snapshot of a request in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ChatProgressUpdate {
    /// Since the request started (before connecting).
    pub elapsed_ms: u64,
    /// When the first output arrived; `None` while the model reads the prompt.
    pub first_output_ms: Option<u64>,
    /// Output received so far, in characters (≈ 4 per token).
    pub output_chars: u64,
    /// True once the model finished; validation follows.
    pub finished: bool,
}

/// Receives progress on the sending thread, at most every 250 ms.
#[uniffi::export(with_foreign)]
pub trait ChatProgress: Send + Sync {
    fn on_progress(&self, update: ChatProgressUpdate);
}

const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Where an article chat goes, plus the progress sink of its one request.
#[derive(Clone)]
struct ChatDestination {
    target: ChatTarget,
    progress: Arc<Mutex<Option<Arc<dyn ChatProgress>>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChatTarget {
    Hosted(wiki_ai::hosted_selection::SelectedHostedModel),
    Local(wiki_ai::local_selection::SelectedLocalModel),
}

impl ChatDestination {
    fn new(target: ChatTarget) -> Self {
        Self {
            target,
            progress: Arc::new(Mutex::new(None)),
        }
    }
}

// Consent binds the destination, never the progress sink.
impl PartialEq for ChatDestination {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
    }
}

impl std::fmt::Debug for ChatDestination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.target.fmt(f)
    }
}

impl wiki_ai::article_destination::ArticleDestination for ChatDestination {
    fn model(&self) -> &str {
        match &self.target {
            ChatTarget::Hosted(selected) => selected.model(),
            ChatTarget::Local(selected) => {
                wiki_ai::article_destination::ArticleDestination::model(selected)
            }
        }
    }

    fn locality(&self) -> wiki_ai::Locality {
        match &self.target {
            ChatTarget::Hosted(selected) => selected.locality(),
            ChatTarget::Local(selected) => selected.locality(),
        }
    }

    fn destination(&self) -> &str {
        match &self.target {
            ChatTarget::Hosted(selected) => selected.destination(),
            ChatTarget::Local(selected) => selected.destination(),
        }
    }

    fn connect(&self) -> Result<Box<dyn wiki_ai::LlmProvider>, wiki_ai::ProviderError> {
        let inner = match &self.target {
            ChatTarget::Hosted(selected) => selected.connect()?,
            ChatTarget::Local(selected) => selected.connect()?,
        };
        let sink = self.progress.lock().ok().and_then(|sink| sink.clone());
        Ok(match sink {
            Some(sink) => Box::new(ProgressProvider { inner, sink }),
            None => inner,
        })
    }
}

/// Reports output counts and timings of the wrapped provider's stream.
struct ProgressProvider {
    inner: Box<dyn wiki_ai::LlmProvider>,
    sink: Arc<dyn ChatProgress>,
}

impl wiki_ai::LlmProvider for ProgressProvider {
    fn locality(&self) -> wiki_ai::Locality {
        self.inner.locality()
    }

    fn capabilities(&self) -> wiki_ai::ProviderCapabilities {
        self.inner.capabilities()
    }

    fn list_models(&self) -> Result<Vec<wiki_ai::ModelInfo>, wiki_ai::ProviderError> {
        self.inner.list_models()
    }

    fn test_connection(&self) -> Result<(), wiki_ai::ProviderError> {
        self.inner.test_connection()
    }

    fn stream(
        &self,
        request: &wiki_ai::GenerateRequest,
        cancellation: &wiki_ai::CancellationToken,
        on_event: &mut dyn FnMut(wiki_ai::StreamEvent) -> Result<(), wiki_ai::ProviderError>,
    ) -> Result<(), wiki_ai::ProviderError> {
        let start = std::time::Instant::now();
        let millis = |at: std::time::Instant| {
            u64::try_from(at.duration_since(start).as_millis()).unwrap_or(u64::MAX)
        };
        let mut update = ChatProgressUpdate {
            elapsed_ms: 0,
            first_output_ms: None,
            output_chars: 0,
            finished: false,
        };
        self.sink.on_progress(update);
        let mut reported = start;
        self.inner.stream(request, cancellation, &mut |event| {
            let now = std::time::Instant::now();
            match &event {
                wiki_ai::StreamEvent::TextDelta(text) => {
                    update.output_chars = update
                        .output_chars
                        .saturating_add(text.chars().count() as u64);
                    let first = update.first_output_ms.is_none();
                    update.first_output_ms.get_or_insert(millis(now));
                    if first || now.duration_since(reported) >= PROGRESS_INTERVAL {
                        update.elapsed_ms = millis(now);
                        self.sink.on_progress(update);
                        reported = now;
                    }
                }
                wiki_ai::StreamEvent::Completed { .. } => {
                    update.elapsed_ms = millis(now);
                    update.finished = true;
                    self.sink.on_progress(update);
                }
            }
            on_event(event)
        })
    }
}

/// One passage that will be sent, shown to the user before consent.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChatPassage {
    pub block_id: String,
    pub heading_path: Vec<String>,
    pub excerpt: String,
}

impl From<&wiki_search::EvidenceHit> for ChatPassage {
    fn from(hit: &wiki_search::EvidenceHit) -> Self {
        Self {
            block_id: hit.block_id.clone(),
            heading_path: hit.heading_path.clone(),
            excerpt: hit.excerpt.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ChatAnswerPart {
    Text {
        text: String,
    },
    /// A validated citation of a passage of the same article revision.
    Citation {
        passage: ChatPassage,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ChatAnswer {
    Supported {
        parts: Vec<ChatAnswerPart>,
    },
    /// The model said the passages do not answer the question.
    InsufficientEvidence,
}

/// A disclosed, not-yet-sent question about one installed article, to a
/// hosted provider or a local model server.
#[derive(uniffi::Object)]
pub struct ArticleChat {
    article: Article,
    selected: ChatDestination,
    policy: wiki_ai::OutboundPolicy,
    /// The passage containing the user's reader selection, always sent.
    selection: Option<wiki_search::EvidenceHit>,
    question: String,
    preview: wiki_ai::article_destination::ArticlePreview<ChatDestination>,
    cancellation: wiki_ai::CancellationToken,
    sent: Mutex<bool>,
    /// Where a supported answer is saved, with the thread and the personal
    /// data generation observed when the preview was prepared.
    history: Option<Arc<ChatHistory>>,
    thread_id: Option<i64>,
    generation: u64,
    prior_turns: Vec<(String, String)>,
    snapshot_id: Option<String>,
}

/// How one question is asked; the same values at preview and at send.
struct QuestionSettings<'a> {
    question: &'a str,
    model: &'a str,
    prior: &'a [(String, String)],
    policy: wiki_ai::OutboundPolicy,
    selection: Option<&'a wiki_search::EvidenceHit>,
}

impl ArticleChat {
    fn settings(&self) -> QuestionSettings<'_> {
        QuestionSettings {
            question: &self.question,
            model: self.preview.model(),
            prior: &self.prior_turns,
            policy: self.policy,
            selection: self.selection.as_ref(),
        }
    }

    fn with_question<T>(
        settings: QuestionSettings<'_>,
        run: impl FnOnce(&wiki_ai::rag::ArticleQuestion<'_>) -> T,
    ) -> T {
        let QuestionSettings {
            question,
            model,
            prior,
            policy,
            selection,
        } = settings;
        let prior_turns: Vec<wiki_ai::rag::PriorTurn<'_>> = prior
            .iter()
            .map(|(question, answer)| wiki_ai::rag::PriorTurn { question, answer })
            .collect();
        run(&wiki_ai::rag::ArticleQuestion {
            question,
            model,
            passage_limit: CHAT_PASSAGE_LIMIT,
            context_word_budget: CHAT_CONTEXT_WORDS,
            selected_evidence: selection,
            max_output_tokens: CHAT_MAX_OUTPUT_TOKENS,
            outbound_policy: policy,
            prior_turns: &prior_turns,
        })
    }
}

#[uniffi::export]
impl ArticleChat {
    /// The host (or local server URL) the passages and question go to.
    pub fn destination(&self) -> String {
        use wiki_ai::article_destination::ArticleDestination;
        self.selected.destination().to_owned()
    }

    pub fn locality(&self) -> ChatLocality {
        use wiki_ai::article_destination::ArticleDestination;
        self.selected.locality().into()
    }

    /// Size of what is sent before the model answers (question, passages
    /// and earlier turns), for estimating how long the model takes to read it.
    pub fn prompt_chars(&self) -> u64 {
        let history: usize = self
            .preview
            .history()
            .iter()
            .map(|(question, answer)| question.chars().count() + answer.chars().count())
            .sum();
        (self.preview.user_message().chars().count() + history) as u64
    }

    /// The most the model may write, in tokens.
    pub fn max_output_tokens(&self) -> u32 {
        CHAT_MAX_OUTPUT_TOKENS
    }

    /// The passage holding the user's reader selection, if one was given.
    pub fn selected_passage(&self) -> Option<ChatPassage> {
        self.selection.as_ref().map(ChatPassage::from)
    }

    pub fn model(&self) -> String {
        self.preview.model().to_owned()
    }

    pub fn article_title(&self) -> String {
        self.article.display_title.clone()
    }

    pub fn revision_id(&self) -> u64 {
        self.article.revision.revision_id
    }

    /// Exactly the passages that `send` would share.
    pub fn passages(&self) -> Vec<ChatPassage> {
        self.preview
            .passages()
            .iter()
            .map(ChatPassage::from)
            .collect()
    }

    /// Stop an in-flight `send` (it returns `Cancelled`).
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Earlier turns of the thread that will be sent before the question
    /// (citation markers removed), oldest first. Disclose them too.
    pub fn prior_turns(&self) -> Vec<ChatTurn> {
        self.preview
            .history()
            .iter()
            .map(|(question, answer)| ChatTurn {
                question: question.clone(),
                answer: answer.clone(),
            })
            .collect()
    }

    /// Send the disclosed request. `share_article_passages` must be the
    /// user's explicit consent to this preview. Usable once. With chat
    /// history, a supported answer is saved and its thread returned.
    pub fn send(
        &self,
        share_article_passages: bool,
        progress: Option<Arc<dyn ChatProgress>>,
    ) -> Result<ChatReply, ChatError> {
        if !share_article_passages {
            return Err(ChatError::ConsentRequired);
        }
        {
            let mut sent = self.sent.lock().map_err(|_| ChatError::InvalidInput)?;
            if *sent {
                return Err(ChatError::AlreadySent);
            }
            *sent = true;
        }
        if self.cancellation.is_cancelled() {
            return Err(ChatError::Cancelled);
        }
        if let Ok(mut sink) = self.selected.progress.lock() {
            *sink = progress;
        }
        let Some(history) = &self.history else {
            let answer = Self::with_question(self.settings(), |question| {
                wiki_ai::article_destination::answer_article_with_preview_display(
                    &self.selected,
                    &self.article,
                    question,
                    &self.preview,
                    true,
                    &self.cancellation,
                )
            })?;
            return Ok(ChatReply {
                answer: display_answer(answer),
                thread_id: None,
            });
        };
        // A fresh connection: the request may take minutes and must not
        // block history reads; SQLite serializes the final write.
        let mut state = history.open()?;
        let outcome = Self::with_question(self.settings(), |question| {
            crate::article_chat::answer_and_save_article(
                &mut state,
                self.generation,
                crate::article_chat::ArticleChatRequest {
                    article: &self.article,
                    question,
                    selected: &self.selected,
                    preview: &self.preview,
                    share_article_passages: true,
                    existing_thread: self.thread_id,
                    evidence_snapshot: self.snapshot_id.as_deref(),
                    cancellation: &self.cancellation,
                },
            )
        })?;
        Ok(match outcome {
            crate::article_chat::SavedArticleChatOutcome::InsufficientEvidence => ChatReply {
                answer: ChatAnswer::InsufficientEvidence,
                thread_id: self.thread_id,
            },
            crate::article_chat::SavedArticleChatOutcome::Supported {
                parts, thread_id, ..
            } => ChatReply {
                answer: display_answer(
                    wiki_ai::article_destination::ArticleDisplayAnswer::Supported(parts),
                ),
                thread_id: Some(thread_id),
            },
        })
    }
}

fn display_answer(answer: wiki_ai::article_destination::ArticleDisplayAnswer) -> ChatAnswer {
    match answer {
        wiki_ai::article_destination::ArticleDisplayAnswer::InsufficientEvidence => {
            ChatAnswer::InsufficientEvidence
        }
        wiki_ai::article_destination::ArticleDisplayAnswer::Supported(parts) => {
            ChatAnswer::Supported {
                parts: parts
                    .into_iter()
                    .map(|part| match part {
                        wiki_ai::rag::ArticleAnswerDisplayPart::Text(text) => {
                            ChatAnswerPart::Text { text }
                        }
                        wiki_ai::rag::ArticleAnswerDisplayPart::Citation(target) => {
                            ChatAnswerPart::Citation {
                                passage: ChatPassage::from(&target.evidence),
                            }
                        }
                    })
                    .collect(),
            }
        }
    }
}

/// The result of a sent question.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChatReply {
    pub answer: ChatAnswer,
    /// The saved thread (new or continued); `None` without chat history or
    /// for an unsaved abstention on a new thread.
    pub thread_id: Option<i64>,
}

/// An earlier exchange resent as context.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChatTurn {
    pub question: String,
    pub answer: String,
}

impl From<crate::article_chat::ArticleChatError> for ChatError {
    fn from(error: crate::article_chat::ArticleChatError) -> Self {
        use crate::article_chat::ArticleChatError as E;
        match error {
            E::Inference(error) => error.into(),
            E::Store(error) => store_error(error),
            E::Cancelled => Self::Cancelled,
            E::InvalidSavedMessage => Self::InvalidInput,
            E::StalePersonalData | E::HistoryMismatch => Self::PreviewChanged,
        }
    }
}

// ---------------------------------------------------------------------------
// Android chat history: exact-revision threads in the shared user-state
// SQLite (the same schema the desktop uses), in an app-private directory.

const MAX_LISTED_THREADS: usize = 200;
const MAX_LISTED_EXCHANGES: usize = 200;

/// One saved part of an answer. Citations are historical: they show the
/// evidence saved with the answer, which may differ from a newer revision.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SavedAnswerPart {
    Text {
        text: String,
    },
    Citation {
        block_id: String,
        passage: Option<ChatPassage>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SavedExchange {
    pub question_id: i64,
    pub answer_id: i64,
    pub question: String,
    pub answer: Vec<SavedAnswerPart>,
}

#[derive(uniffi::Object)]
pub struct ChatHistory {
    database: PathBuf,
}

impl ChatHistory {
    fn open(&self) -> Result<wiki_store::user_state::UserState, ChatError> {
        wiki_store::user_state::UserState::open(&self.database).map_err(|_| ChatError::Storage)
    }

    fn generation(&self) -> Result<u64, ChatError> {
        self.open()?
            .personal_generation()
            .map_err(|_| ChatError::Storage)
    }
}

fn article_key(project: String, page_id: u64) -> Result<ArticleKey, ChatError> {
    let key = ArticleKey { project, page_id };
    key.validate().map_err(|_| ChatError::InvalidInput)?;
    Ok(key)
}

fn store_error(error: wiki_store::user_state::UserStateError) -> ChatError {
    match error {
        wiki_store::user_state::UserStateError::InvalidValue => ChatError::InvalidInput,
        _ => ChatError::Storage,
    }
}

/// Split stored answer text at its `[[cite:BLOCK]]` markers.
fn saved_parts(
    text: &str,
    citations: &[wiki_store::user_state::StoredCitation],
) -> Vec<SavedAnswerPart> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[cite:") {
        let after = &rest[start + 7..];
        let Some(end) = after.find("]]") else {
            break;
        };
        if start > 0 {
            parts.push(SavedAnswerPart::Text {
                text: rest[..start].to_owned(),
            });
        }
        let block_id = after[..end].to_owned();
        let passage = citations
            .iter()
            .find(|citation| citation.block_id == block_id)
            .map(|citation| ChatPassage {
                block_id: citation.block_id.clone(),
                heading_path: citation.heading_path.clone(),
                excerpt: citation.excerpt.clone(),
            });
        parts.push(SavedAnswerPart::Citation { block_id, passage });
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        parts.push(SavedAnswerPart::Text {
            text: rest.to_owned(),
        });
    }
    parts
}

#[uniffi::export]
impl ChatHistory {
    /// Open (creating if needed) chat history in an absolute app-private
    /// directory (Android: under `noBackupFilesDir`).
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, ChatError> {
        let requested = PathBuf::from(&data_dir);
        if !requested.is_absolute() || data_dir.contains('\0') {
            return Err(ChatError::InvalidInput);
        }
        std::fs::create_dir_all(&requested).map_err(|_| ChatError::Storage)?;
        let root = std::fs::canonicalize(&requested).map_err(|_| ChatError::Storage)?;
        let history = Self {
            database: root.join("chat-history.sqlite"),
        };
        history.open()?;
        Ok(Arc::new(history))
    }

    /// Thread IDs saved for one exact article revision, newest first.
    pub fn threads(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
    ) -> Result<Vec<i64>, ChatError> {
        let key = article_key(project, page_id)?;
        self.open()?
            .list_chat_threads(&key, revision_id, MAX_LISTED_THREADS)
            .map_err(store_error)
    }

    /// The saved exchanges of a thread, oldest first.
    pub fn exchanges(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
        thread_id: i64,
    ) -> Result<Vec<SavedExchange>, ChatError> {
        let key = article_key(project, page_id)?;
        let state = self.open()?;
        state
            .recent_chat_exchanges(&key, revision_id, thread_id, MAX_LISTED_EXCHANGES)
            .map_err(store_error)?
            .into_iter()
            .map(|exchange| {
                let citations = state
                    .chat_citations(&key, revision_id, thread_id, exchange.answer.id)
                    .map_err(store_error)?;
                Ok(SavedExchange {
                    question_id: exchange.question.id,
                    answer_id: exchange.answer.id,
                    question: exchange.question.content,
                    answer: saved_parts(&exchange.answer.content, &citations),
                })
            })
            .collect()
    }

    /// Delete one thread and its messages. Returns whether it existed.
    pub fn delete_thread(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
        thread_id: i64,
    ) -> Result<bool, ChatError> {
        let key = article_key(project, page_id)?;
        let state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        state
            .delete_chat_thread_for_generation(generation, &key, revision_id, thread_id)
            .map_err(store_error)
    }

    /// Delete every saved chat. Questions prepared earlier can no longer be
    /// saved (they fail with `PreviewChanged`).
    pub fn clear_all(&self) -> Result<(), ChatError> {
        let mut state = self.open()?;
        let generation = state.personal_generation().map_err(store_error)?;
        if state
            .clear_personal_reading_data_for_generation(generation)
            .map_err(store_error)?
        {
            Ok(())
        } else {
            Err(ChatError::PreviewChanged)
        }
    }
}

/// The article-side half of a chat request, shared by every destination.
struct ChatQuestion {
    project: String,
    page_id: u64,
    question: String,
    thread_id: Option<i64>,
    selection: Option<String>,
}

const MAX_SELECTION_BYTES: usize = 4096;

impl OfflineLibrary {
    fn prepare_chat(
        &self,
        ask: ChatQuestion,
        selected: ChatDestination,
        policy: wiki_ai::OutboundPolicy,
        history: Option<Arc<ChatHistory>>,
    ) -> Result<Arc<ArticleChat>, ChatError> {
        let ChatQuestion {
            project,
            page_id,
            question,
            thread_id,
            selection,
        } = ask;
        if thread_id.is_some() && history.is_none() {
            return Err(ChatError::InvalidInput);
        }
        let question = question.trim().to_owned();
        if question.is_empty() || question.len() > MAX_CHAT_QUESTION_BYTES {
            return Err(ChatError::InvalidInput);
        }
        if selection
            .as_ref()
            .is_some_and(|text| text.len() > MAX_SELECTION_BYTES)
        {
            return Err(ChatError::InvalidInput);
        }
        let key = ArticleKey { project, page_id };
        key.validate().map_err(|_| ChatError::InvalidInput)?;
        let article = self
            .with_active(|reader| {
                reader
                    .reader()
                    .open_page(&key)
                    .map_err(|_| LibraryError::VerificationFailed)
            })
            .map_err(|error| match error {
                LibraryError::NotInstalled => ChatError::NotInstalled,
                LibraryError::InvalidInput => ChatError::InvalidInput,
                _ => ChatError::VerificationFailed,
            })?
            .ok_or(ChatError::NotInstalled)?;
        // A selection is resolved to the one passage containing it, from the
        // verified article itself: the app's text is never sent as evidence.
        let selection = match selection.filter(|text| !text.trim().is_empty()) {
            Some(text) => Some(
                wiki_search::ArticleLexicalIndex::build(&article)
                    .map_err(|_| ChatError::VerificationFailed)?
                    .evidence_containing(&text)
                    .ok_or(ChatError::SelectionNotFound)?,
            ),
            None => None,
        };
        let snapshot_id = self.active_snapshot().map_err(|_| ChatError::Storage)?;
        // Capture the privacy generation BEFORE reading the thread, so a
        // concurrent "clear all" makes the later save fail closed.
        let (generation, prior_turns) = match &history {
            Some(history) => {
                let generation = history.generation()?;
                let prior = match thread_id {
                    Some(id) => {
                        let state = history.open()?;
                        crate::article_chat::prior_turns_for_thread(&state, &article, id)
                            .map_err(ChatError::from)?
                    }
                    None => Vec::new(),
                };
                (generation, prior)
            }
            None => (0, Vec::new()),
        };
        let model = wiki_ai::article_destination::ArticleDestination::model(&selected).to_owned();
        let settings = QuestionSettings {
            question: &question,
            model: &model,
            prior: &prior_turns,
            policy,
            selection: selection.as_ref(),
        };
        let preview = ArticleChat::with_question(settings, |question| {
            wiki_ai::article_destination::prepare_article_preview(&selected, &article, question)
        })?
        .ok_or(ChatError::NoEvidence)?;
        Ok(Arc::new(ArticleChat {
            article,
            selected,
            policy,
            selection,
            question,
            preview,
            cancellation: wiki_ai::CancellationToken::default(),
            sent: Mutex::new(false),
            history,
            thread_id,
            generation,
            prior_turns,
            snapshot_id,
        }))
    }
}

#[uniffi::export]
impl OfflineLibrary {
    /// Prepare (no network) a hosted question about an article of the active
    /// pack. `api_key` comes from the app's secure storage and is kept only
    /// in memory by the returned object.
    pub fn prepare_hosted_chat(
        &self,
        request: HostedChatRequest,
        api_key: String,
        history: Option<Arc<ChatHistory>>,
    ) -> Result<Arc<ArticleChat>, ChatError> {
        let HostedChatRequest {
            project,
            page_id,
            question,
            provider,
            model,
            thread_id,
            selection,
        } = request;
        let key = wiki_ai::hosted_codec::ApiKey::new(api_key).map_err(|_| ChatError::InvalidKey)?;
        let selected =
            wiki_ai::hosted_selection::SelectedHostedModel::new(provider.into(), model.trim(), key)
                .map_err(|_| ChatError::InvalidInput)?;
        self.prepare_chat(
            ChatQuestion {
                project,
                page_id,
                question,
                thread_id,
                selection,
            },
            ChatDestination::new(ChatTarget::Hosted(selected)),
            wiki_ai::OutboundPolicy::AllowCloud,
            history,
        )
    }

    /// Prepare (no network) a question for a model of a discovered local
    /// server. Only that server's listed models can be chosen, and the
    /// request may go only to a loopback or private-network address.
    pub fn prepare_local_chat(
        &self,
        request: LocalChatRequest,
        history: Option<Arc<ChatHistory>>,
    ) -> Result<Arc<ArticleChat>, ChatError> {
        let LocalChatRequest {
            project,
            page_id,
            question,
            server,
            model,
            thread_id,
            selection,
        } = request;
        let selected = server
            .inventory
            .select(model.trim())
            .map_err(|_| ChatError::InvalidInput)?;
        self.prepare_chat(
            ChatQuestion {
                project,
                page_id,
                question,
                thread_id,
                selection,
            },
            ChatDestination::new(ChatTarget::Local(selected)),
            wiki_ai::OutboundPolicy::AllowLocalEndpoints,
            history,
        )
    }
}

// ---------------------------------------------------------------------------
// MOB-002 article reader: script-free, re-sanitized documents for the
// article WebView, opened by page ID (search) or by title (in-app links).

/// A verified article revision ready for the article WebView.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReaderPage {
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    pub title: String,
    pub display_title: String,
    /// Complete sanitized HTML document (see `wiki_source::reader_document`).
    pub document: String,
    /// The document's URL in the reserved reader origin (title
    /// percent-encoded). The app serves `document` for exactly this URL.
    pub base_url: String,
}

/// Percent-encode a title as a URL path (spaces become `_`, as on
/// Wikipedia), so `?`, `#`, `%` and non-ASCII never change the URL's meaning.
fn encode_title_path(title: &str) -> String {
    let mut path = String::with_capacity(title.len());
    for byte in title.replace(' ', "_").bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:()!,'*".contains(&byte) {
            path.push(byte as char);
        } else {
            path.push_str(&format!("%{byte:02X}"));
        }
    }
    path
}

fn reader_theme(dark: bool) -> wiki_source::reader_document::ReaderTheme {
    use wiki_source::reader_document::ReaderTheme;
    if dark {
        ReaderTheme::Dark
    } else {
        ReaderTheme::Light
    }
}

fn reader_page(article: Article, dark: bool) -> ReaderPage {
    let document = wiki_source::reader_document::reader_document(&article, reader_theme(dark));
    reader_page_with(article, document)
}

fn reader_page_with(article: Article, document: String) -> ReaderPage {
    use wiki_source::reader_document::{ARTICLE_PATH, READER_ORIGIN};
    ReaderPage {
        base_url: format!(
            "{READER_ORIGIN}{ARTICLE_PATH}{}",
            encode_title_path(&article.title)
        ),
        project: article.key.project,
        page_id: article.key.page_id,
        revision_id: article.revision.revision_id,
        title: article.title,
        display_title: article.display_title,
        document,
    }
}

/// A reader document with one cited passage highlighted (MOB-003).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CitedReaderPage {
    pub page: ReaderPage,
    /// Load `page.base_url#fragment` to scroll to the highlighted passage.
    pub fragment: String,
    /// True when the paragraph itself is highlighted in the article text;
    /// false when its text is shown, labelled, at the top of the page
    /// because it matched no single paragraph of the rendered article.
    pub located: bool,
}

#[uniffi::export]
impl OfflineLibrary {
    /// Open the article revision a chat citation points to, with the cited
    /// passage highlighted. `None` if the active pack does not hold exactly
    /// that revision or the passage is not one of its blocks: a citation
    /// is never shown against different text.
    pub fn open_reader_citation(
        &self,
        project: String,
        page_id: u64,
        revision_id: u64,
        block_id: String,
        dark: bool,
    ) -> Result<Option<CitedReaderPage>, LibraryError> {
        use wiki_source::reader_document::{reader_document_with_citation, CitedPassage};
        let key = ArticleKey { project, page_id };
        key.validate().map_err(|_| LibraryError::InvalidInput)?;
        if block_id.is_empty() || block_id.len() > 512 {
            return Err(LibraryError::InvalidInput);
        }
        let article = self.with_active(|reader| {
            reader
                .reader()
                .open_page(&key)
                .map_err(|_| LibraryError::VerificationFailed)
        })?;
        let Some(article) = article.filter(|article| article.revision.revision_id == revision_id)
        else {
            return Ok(None);
        };
        let Some((document, passage)) =
            reader_document_with_citation(&article, reader_theme(dark), &block_id)
        else {
            return Ok(None);
        };
        Ok(Some(CitedReaderPage {
            page: reader_page_with(article, document),
            fragment: wiki_source::reader_document::CITED_FRAGMENT.to_owned(),
            located: passage == CitedPassage::Located,
        }))
    }

    /// Open an article of the active pack by page ID as a reader document.
    pub fn open_reader_page(
        &self,
        project: String,
        page_id: u64,
        dark: bool,
    ) -> Result<Option<ReaderPage>, LibraryError> {
        let key = ArticleKey { project, page_id };
        key.validate().map_err(|_| LibraryError::InvalidInput)?;
        self.with_active(|reader| {
            reader
                .reader()
                .open_page(&key)
                .map_err(|_| LibraryError::VerificationFailed)
        })
        .map(|article| article.map(|article| reader_page(article, dark)))
    }

    /// Resolve an in-app article link (a title or installed redirect) within
    /// the active pack. `None` means the article is not in this pack.
    pub fn open_reader_title(
        &self,
        project: String,
        title: String,
        dark: bool,
    ) -> Result<Option<ReaderPage>, LibraryError> {
        let probe = ArticleKey {
            project: project.clone(),
            page_id: 1,
        };
        probe.validate().map_err(|_| LibraryError::InvalidInput)?;
        if !wiki_model::valid_navigation_title(&title) {
            return Err(LibraryError::InvalidInput);
        }
        self.with_active(|reader| {
            reader
                .reader()
                .open_title(&project, &title)
                .map_err(|_| LibraryError::VerificationFailed)
        })
        .map(|article| article.map(|article| reader_page(article, dark)))
    }
}

#[cfg(test)]
mod reader_url_tests {
    use super::encode_title_path;

    #[test]
    fn titles_are_encoded_so_they_cannot_become_queries_or_fragments() {
        assert_eq!(encode_title_path("Isaac Newton"), "Isaac_Newton");
        assert_eq!(encode_title_path("AC/DC"), "AC/DC");
        assert_eq!(encode_title_path("C#"), "C%23");
        assert_eq!(encode_title_path("What? 100%"), "What%3F_100%25");
        assert_eq!(encode_title_path("Éclair"), "%C3%89clair");
    }
}
