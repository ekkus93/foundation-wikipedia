//! STORE-003 custom `.wpack` installer: stage, verify, confirm, publish.
//!
//! A pack's objects mirror the snapshot layout directly:
//! `catalog.sqlite`, the article shards the catalog names, and optionally a
//! derived `search.sqlite`. The installer
//!
//! 1. stages the envelope through `wiki_pack_format::stage` into the private
//!    `<root>/staging/` directory (two-pass digest verification, 0700/0600);
//! 2. accepts an `Origin::Official` claim only through `install_official_pack*`
//!    with a publisher signature that a pinned key verifies over the staged
//!    manifest (TRUST-001); an official label is never earned by hashes alone;
//! 3. checks the layout, treats every other object as a manifest-pinned shard,
//!    rebuilds an omitted search index from those verified shards, and opens
//!    the staged tree through `VerifiedOfflineCatalogReader`;
//! 4. writes the installer-owned `manifest.json`, then asks the caller to
//!    confirm the reviewed, integrity-verified custom pack;
//! 5. renames the staged tree to `snapshots/<pack_id>` (same filesystem) and
//!    syncs both directories.
//!
//! Every failure before step 5, including a declined confirmation, removes the
//! staged tree durably. Installation never changes the `current` selector:
//! activation remains a separate `ActiveSnapshotStore` switch, which applies
//! its own candidate durability barrier.
//!
//! Preconditions: the caller owns `<root>` exclusively for the duration of the
//! call, and `staging/` and `snapshots/` are on the same filesystem as each
//! other (they are siblings). Pack media are not installed here; a pack
//! containing objects the catalog does not name is refused.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use wiki_pack_format::container::ContainerError;
use wiki_pack_format::manifest::{
    Counts, IndexEntry, IndexRole, License, Manifest, Origin, PackMetadata, SourceSnapshot,
};
use wiki_pack_format::signature::{self, PackSignature, SignatureError, TrustedKey};
use wiki_pack_format::stage;
use wiki_pack_format::trust::{classify, IntegrityState, TrustStatus};

use crate::activation::safe_id;
use crate::catalog::SnapshotCatalog;
use crate::offline_reader::{ReaderError, VerifiedOfflineCatalogReader};
use crate::pack_media::{
    check_pack_media, is_media_object, parse_media_index, required_images, PackMediaError,
    MEDIA_INDEX_OBJECT,
};
use crate::search_index::{OfflineSearchIndex, SearchError};

pub const CATALOG_OBJECT: &str = "catalog.sqlite";
pub const SEARCH_OBJECT: &str = "search.sqlite";
pub const MANIFEST_FILE: &str = "manifest.json";
/// Index schema versions this installer understands (manifest v2 `indexes`).
pub const CATALOG_SCHEMA: u32 = 1;
pub const FTS5_SCHEMA: u32 = 1;

static NEXT_STAGE_ID: AtomicU64 = AtomicU64::new(0);

/// Shard inventory pinned by the verified pack manifest, in the shape the
/// manifest-verified activation and reader APIs expect.
pub type ShardInventory = BTreeMap<String, (u64, String)>;

/// What the user is asked to confirm: a structurally verified custom pack.
#[derive(Debug)]
pub struct PackInstallReview<'a> {
    pub manifest: &'a Manifest,
    pub trust: TrustStatus,
    pub declared_bytes: u64,
    /// The pack shipped no search index; one was built from verified shards.
    pub rebuilt_search_index: bool,
}

#[derive(Debug)]
pub struct InstalledPack {
    pub snapshot_id: String,
    pub path: PathBuf,
    pub manifest: Manifest,
    pub trust: TrustStatus,
    pub shards: ShardInventory,
}

#[derive(Debug)]
pub enum PackInstallError {
    Io(io::Error),
    UnsafeRoot,
    Container(ContainerError),
    /// An official-origin claim without a publisher signature.
    UnsignedOfficialPack,
    /// The publisher signature did not verify against a pinned key.
    PublisherVerificationFailed(SignatureError),
    /// A publisher signature was supplied for a pack that is not official.
    NotOfficialPack,
    /// `pack_id` is not a valid snapshot directory name.
    InvalidSnapshotId,
    /// An object the snapshot layout reserves or cannot use.
    UnsupportedObject(String),
    MissingCatalog,
    Search(SearchError),
    Reader(ReaderError),
    AlreadyInstalled,
    Declined,
    /// The snapshot is the active selection and cannot be removed.
    SnapshotActive,
    /// No installed snapshot has this ID.
    NotInstalled,
    /// The pack's images are incomplete, unused, mislabelled or damaged.
    Media(PackMediaError),
    /// Declared article/redirect counts disagree with the verified catalog.
    CountMismatch {
        declared: Counts,
        actual: Counts,
    },
}

impl From<io::Error> for PackInstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Install a seekable custom `.wpack` whose whole envelope fits
/// `max_input_bytes`. `confirm` runs only after full verification and must
/// return `true` for the user's explicit acceptance of the custom pack.
pub fn install_custom_pack<R: Read + Seek>(
    root: &Path,
    input: &mut R,
    max_input_bytes: u64,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
) -> Result<InstalledPack, PackInstallError> {
    install_with(root, None, confirm, |parent, name| {
        stage::stage_container_with_budget(input, parent, name, max_input_bytes)
    })
}

/// The publisher signature an official pack is installed with, and the
/// pinned keys it must verify against (`signature::OFFICIAL_KEYS` in the app).
#[derive(Clone, Copy, Debug)]
pub struct PublisherProof<'a> {
    pub signature: &'a PackSignature,
    pub keyring: &'a [TrustedKey],
}

/// Install a seekable official `.wpack`: as `install_custom_pack`, but the
/// staged manifest must claim official origin and `proof` must verify it.
pub fn install_official_pack<R: Read + Seek>(
    root: &Path,
    input: &mut R,
    max_input_bytes: u64,
    proof: PublisherProof<'_>,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
) -> Result<InstalledPack, PackInstallError> {
    install_with(root, Some(proof), confirm, |parent, name| {
        stage::stage_container_with_budget(input, parent, name, max_input_bytes)
    })
}

/// `install_official_pack` for an unseekable stream (a download).
pub fn install_official_pack_stream<R: Read>(
    root: &Path,
    input: &mut R,
    max_spool_bytes: u64,
    proof: PublisherProof<'_>,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
) -> Result<InstalledPack, PackInstallError> {
    install_with(root, Some(proof), confirm, |parent, name| {
        stage::stage_container_stream_with_budget(input, parent, name, max_spool_bytes)
    })
}

/// Install an unseekable stream (for example an Android document provider)
/// through a private spool bounded by `max_spool_bytes`.
pub fn install_custom_pack_stream<R: Read>(
    root: &Path,
    input: &mut R,
    max_spool_bytes: u64,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
) -> Result<InstalledPack, PackInstallError> {
    install_with(root, None, confirm, |parent, name| {
        stage::stage_container_stream_with_budget(input, parent, name, max_spool_bytes)
    })
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        result => result,
    }
}

fn real_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.is_dir() && !meta.file_type().is_symlink()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn prepare_root(root: &Path) -> Result<(PathBuf, PathBuf), PackInstallError> {
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
            return Err(PackInstallError::UnsafeRoot);
        }
    }
    if !real_directory(root)? {
        return Err(PackInstallError::UnsafeRoot);
    }
    let snapshots = root.join("snapshots");
    let staging = root.join("staging");
    for dir in [&snapshots, &staging] {
        create_private_dir(dir)?;
        if !real_directory(dir)? {
            return Err(PackInstallError::UnsafeRoot);
        }
    }
    Ok((snapshots, staging))
}

fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// Remove staging residue an interrupted install or removal left behind:
/// `install-*` and `remove-*` directories and `.wpack-spool-*` files in the private `staging/`
/// directory. Anything else there is left untouched and counted as retained.
///
/// The caller MUST guarantee no install is running against `root` (the same
/// exclusive-ownership precondition the installer has), for example by
/// calling this once at startup before any install. A staged tree is never
/// adopted: published snapshots live only under `snapshots/`, which this
/// never touches. Returns `(removed, retained)`.
pub fn reclaim_abandoned_staging(root: &Path) -> Result<(usize, usize), PackInstallError> {
    let (_, staging) = prepare_root(root)?;
    let mut removed = 0;
    let mut retained = 0;
    for entry in fs::read_dir(&staging)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // file_type does not follow symlinks.
        let kind = entry.file_type()?;
        if (name.starts_with("install-") || name.starts_with("remove-")) && kind.is_dir() {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        } else if name.starts_with(".wpack-spool-") && kind.is_file() {
            fs::remove_file(entry.path())?;
            removed += 1;
        } else {
            retained += 1;
        }
    }
    if removed > 0 {
        sync_dir(&staging)?;
    }
    Ok((removed, retained))
}

fn install_with(
    root: &Path,
    proof: Option<PublisherProof<'_>>,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
    stage_into: impl FnOnce(&Path, &str) -> Result<(Manifest, PathBuf), ContainerError>,
) -> Result<InstalledPack, PackInstallError> {
    install_with_sync(root, proof, confirm, stage_into, sync_dir)
}

// Injectable directory-sync seam for fault tests of publication.
fn install_with_sync(
    root: &Path,
    proof: Option<PublisherProof<'_>>,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
    stage_into: impl FnOnce(&Path, &str) -> Result<(Manifest, PathBuf), ContainerError>,
    mut sync: impl FnMut(&Path) -> io::Result<()>,
) -> Result<InstalledPack, PackInstallError> {
    let (snapshots, staging) = prepare_root(root)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let name = format!(
        "install-{}-{nonce}-{}",
        std::process::id(),
        NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    );
    let (manifest, staged) = stage_into(&staging, &name).map_err(PackInstallError::Container)?;

    let prepared = prepare_staged(&manifest, proof, &staged, &snapshots, confirm, &mut sync);
    let (id, trust, shards) = match prepared {
        Ok(prepared) => prepared,
        Err(cause) => return Err(rollback(&staging, &staged, cause, &mut sync)),
    };
    let target = snapshots.join(&id);
    if let Err(error) = fs::rename(&staged, &target) {
        return Err(rollback(&staging, &staged, error.into(), &mut sync));
    }
    // The snapshot is now published but inactive. A later sync failure is
    // reported; a retry reports AlreadyInstalled and activation re-syncs the
    // whole candidate tree before the selector can name it.
    sync(&snapshots)?;
    sync(&staging)?;
    Ok(InstalledPack {
        snapshot_id: id,
        path: target,
        manifest,
        trust,
        shards,
    })
}

fn rollback(
    staging: &Path,
    staged: &Path,
    cause: PackInstallError,
    sync: &mut impl FnMut(&Path) -> io::Result<()>,
) -> PackInstallError {
    if let Err(error) = fs::remove_dir_all(staged) {
        return PackInstallError::Io(io::Error::other(format!(
            "install failed: {cause:?}; staged tree removal failed: {error}"
        )));
    }
    if let Err(error) = sync(staging) {
        return PackInstallError::Io(io::Error::other(format!(
            "install failed: {cause:?}; staging sync failed: {error}"
        )));
    }
    cause
}

fn prepare_staged(
    manifest: &Manifest,
    proof: Option<PublisherProof<'_>>,
    staged: &Path,
    snapshots: &Path,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
    sync: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<(String, TrustStatus, ShardInventory), PackInstallError> {
    manifest
        .validate()
        .map_err(|error| PackInstallError::Container(ContainerError::Manifest(error)))?;
    // Staging rehashed every object byte against this manifest, so a valid
    // signature over it authenticates every installed byte.
    let (trust, signing_key) = match (&manifest.origin, proof) {
        (Origin::Custom { .. }, None) => (classify(manifest, IntegrityState::Passed), None),
        (Origin::Custom { .. }, Some(_)) => return Err(PackInstallError::NotOfficialPack),
        (Origin::Official { .. }, None) => return Err(PackInstallError::UnsignedOfficialPack),
        (Origin::Official { .. }, Some(proof)) => {
            signature::verify(manifest, proof.signature, proof.keyring)
                .map_err(PackInstallError::PublisherVerificationFailed)?;
            (TrustStatus::VerifiedPublisher, Some(proof.signature.key_id))
        }
    };
    let id = manifest.pack_id.clone();
    if !safe_id(&id) {
        return Err(PackInstallError::InvalidSnapshotId);
    }

    let metadata = &manifest.metadata;
    // Index objects are located from the manifest's declared roles; the
    // snapshot layout fixes their paths and supported schema versions.
    let mut has_catalog = false;
    let mut has_search = false;
    for index in &metadata.indexes {
        let (path, schema) = match index.role {
            IndexRole::Catalog => (CATALOG_OBJECT, CATALOG_SCHEMA),
            IndexRole::Fts5 => (SEARCH_OBJECT, FTS5_SCHEMA),
        };
        if index.path != path || index.schema != schema {
            return Err(PackInstallError::UnsupportedObject(index.path.clone()));
        }
        match index.role {
            IndexRole::Catalog => has_catalog = true,
            IndexRole::Fts5 => has_search = true,
        }
    }
    if !has_catalog {
        return Err(PackInstallError::MissingCatalog);
    }
    let attribution = metadata.license.attribution_object.as_deref();
    let mut shards = ShardInventory::new();
    let mut declared_bytes = 0u64;
    for object in &manifest.objects {
        declared_bytes = declared_bytes.saturating_add(object.bytes);
        let path = object.path.as_str();
        let sqlite_sidecar = ["-wal", "-shm", "-journal"]
            .iter()
            .any(|suffix| path.ends_with(suffix));
        if path == MANIFEST_FILE || sqlite_sidecar || path.starts_with(".offline-fts.") {
            return Err(PackInstallError::UnsupportedObject(object.path.clone()));
        }
        if path == CATALOG_OBJECT || (path == SEARCH_OBJECT && has_search) {
            continue;
        }
        // An undeclared index file is not canonical content.
        if path == SEARCH_OBJECT {
            return Err(PackInstallError::UnsupportedObject(object.path.clone()));
        }
        // Images are checked against the articles below, not as shards.
        if Some(path) == attribution || is_media_object(path) {
            continue;
        }
        shards.insert(
            object.path.clone(),
            (object.bytes, object.sha256.to_ascii_lowercase()),
        );
    }

    let catalog_path = staged.join(CATALOG_OBJECT);
    let search_path = staged.join(SEARCH_OBJECT);
    if !has_search {
        let catalog = SnapshotCatalog::open_read_only(&catalog_path)
            .map_err(|error| PackInstallError::Reader(ReaderError::Catalog(error)))?;
        OfflineSearchIndex::replace_persisted_from_verified_shards(
            &search_path,
            &catalog,
            staged,
            &shards,
        )
        .map_err(PackInstallError::Search)?;
    }
    // Full verified opening: catalog coverage equals the pinned shard set,
    // every shard rehashes, and the index matches the catalog.
    let reader = VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &catalog_path,
        &search_path,
        staged,
        &shards,
    )
    .map_err(PackInstallError::Reader)?;
    drop(reader);
    let (actual, required, without_bytes) = scan_catalog_records(&catalog_path, staged)?;
    check_pack_media(
        staged,
        &manifest.objects,
        attribution,
        metadata.counts.media,
        &required,
        &without_bytes,
    )
    .map_err(PackInstallError::Media)?;
    if actual.articles != metadata.counts.articles || actual.redirects != metadata.counts.redirects
    {
        return Err(PackInstallError::CountMismatch {
            declared: metadata.counts,
            actual,
        });
    }

    write_snapshot_manifest(staged, manifest, trust, signing_key)?;
    sync(staged)?;

    match fs::symlink_metadata(snapshots.join(&id)) {
        Ok(_) => return Err(PackInstallError::AlreadyInstalled),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let review = PackInstallReview {
        manifest,
        trust,
        declared_bytes,
        rebuilt_search_index: !has_search,
    };
    if !confirm(&review) {
        return Err(PackInstallError::Declined);
    }
    Ok((id, trust, shards))
}

/// Count verified catalog records by kind, decoding one frame at a time.
fn count_catalog_records(catalog_path: &Path, root: &Path) -> Result<Counts, PackInstallError> {
    scan_catalog_records(catalog_path, root).map(|(counts, _, _)| counts)
}

/// Counts, the images articles require (digest → format) and the titles of
/// articles with an image that has no bytes, from one verified scan.
type CatalogScan = (Counts, BTreeMap<String, String>, Vec<String>);

fn scan_catalog_records(catalog_path: &Path, root: &Path) -> Result<CatalogScan, PackInstallError> {
    let catalog = SnapshotCatalog::open_read_only(catalog_path)
        .map_err(|error| PackInstallError::Reader(ReaderError::Catalog(error)))?;
    let mut counts = Counts::default();
    let mut required = BTreeMap::new();
    let mut without_bytes = Vec::new();
    let mut conflict = None;
    catalog
        .visit_records(root, |record| {
            match record {
                wiki_model::PageRecord::Article(article) => {
                    counts.articles += 1;
                    match required_images([&*article]) {
                        Ok((images, missing)) => {
                            for (digest, mime) in images {
                                if let Some(previous) =
                                    required.insert(digest.clone(), mime.clone())
                                {
                                    if previous != mime {
                                        conflict = Some(digest);
                                    }
                                }
                            }
                            without_bytes.extend(missing);
                        }
                        Err(PackMediaError::MissingImage(digest)) => conflict = Some(digest),
                        Err(_) => {}
                    }
                }
                wiki_model::PageRecord::Redirect(_) => counts.redirects += 1,
            }
            Ok::<(), crate::catalog::CatalogError>(())
        })
        .map_err(|error| PackInstallError::Reader(ReaderError::Catalog(error)))?;
    if let Some(digest) = conflict {
        return Err(PackInstallError::Media(PackMediaError::MissingImage(
            digest,
        )));
    }
    Ok((counts, required, without_bytes))
}

/// Derive manifest v2 metadata for a snapshot-layout tree: the declared
/// indexes come from the files present (`catalog.sqlite` required,
/// `search.sqlite` optional) and the counts from a verified catalog scan.
/// Source, definition hash and text licence are the builder's claims and
/// are passed through. A tree with [`MEDIA_INDEX_OBJECT`] declares it as the
/// attribution object and counts its images (PACK-003).
pub fn snapshot_pack_metadata(
    tree: &Path,
    source: SourceSnapshot,
    definition_sha256: &str,
    text_spdx: &str,
) -> Result<PackMetadata, PackInstallError> {
    let catalog_path = tree.join(CATALOG_OBJECT);
    if !catalog_path.is_file() {
        return Err(PackInstallError::MissingCatalog);
    }
    let mut indexes = vec![IndexEntry {
        path: CATALOG_OBJECT.into(),
        role: IndexRole::Catalog,
        schema: CATALOG_SCHEMA,
    }];
    if tree.join(SEARCH_OBJECT).is_file() {
        indexes.push(IndexEntry {
            path: SEARCH_OBJECT.into(),
            role: IndexRole::Fts5,
            schema: FTS5_SCHEMA,
        });
    }
    let mut counts = count_catalog_records(&catalog_path, tree)?;
    let index = tree.join(MEDIA_INDEX_OBJECT);
    let attribution_object = if index.is_file() {
        let images = parse_media_index(&fs::read(&index)?).map_err(PackInstallError::Media)?;
        counts.media = images.len() as u64;
        Some(MEDIA_INDEX_OBJECT.to_owned())
    } else {
        None
    };
    Ok(PackMetadata {
        source,
        definition_sha256: definition_sha256.into(),
        license: License {
            text_spdx: text_spdx.into(),
            attribution_object,
        },
        indexes,
        counts,
    })
}

const MAX_INSTALLED_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

/// One installed snapshot as recorded by the installer's `manifest.json`.
/// `damaged` marks a snapshot whose manifest cannot be read; it is listed
/// (never hidden) so the user can remove it, and it can never be activated
/// because verified opening fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledSnapshotInfo {
    pub snapshot_id: String,
    pub project: String,
    pub trust: TrustStatus,
    pub articles: u64,
    pub damaged: bool,
}

/// List installed snapshots in ID order. Never adopts staging residue.
/// A "Verified publisher" label is kept only while its signing key is still
/// pinned and unrevoked in `signature::OFFICIAL_KEYS`.
pub fn list_installed_snapshots(
    root: &Path,
) -> Result<Vec<InstalledSnapshotInfo>, PackInstallError> {
    list_installed_snapshots_with_keyring(root, signature::OFFICIAL_KEYS)
}

/// The status recorded at install, rechecked against `keyring`. An
/// unrecognised record, or a publisher key that is no longer pinned and
/// unrevoked, yields `VerificationFailed`.
fn current_trust(value: &serde_json::Value, recorded: &str, keyring: &[TrustedKey]) -> TrustStatus {
    match TrustStatus::from_installed_label(recorded) {
        Some(TrustStatus::VerifiedPublisher) => {
            let official = &value["origin"]["official"];
            let still_trusted = official["key_id"].as_str().is_some_and(|key_id| {
                keyring.iter().any(|key| {
                    !key.revoked
                        && Some(key.publisher) == official["publisher"].as_str()
                        && signature::hex(&key.key_id()) == key_id
                })
            });
            if still_trusted {
                TrustStatus::VerifiedPublisher
            } else {
                TrustStatus::VerificationFailed
            }
        }
        Some(status) => status,
        None => TrustStatus::VerificationFailed,
    }
}

pub fn list_installed_snapshots_with_keyring(
    root: &Path,
    keyring: &[TrustedKey],
) -> Result<Vec<InstalledSnapshotInfo>, PackInstallError> {
    let (snapshots, _) = prepare_root(root)?;
    let mut listed = Vec::new();
    for entry in fs::read_dir(&snapshots)? {
        let entry = entry?;
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !entry.file_type()?.is_dir() || !safe_id(&id) {
            continue;
        }
        let parsed = fs::symlink_metadata(entry.path().join(MANIFEST_FILE))
            .ok()
            .filter(|meta| meta.is_file() && meta.len() <= MAX_INSTALLED_MANIFEST_BYTES)
            .and_then(|_| fs::read(entry.path().join(MANIFEST_FILE)).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                Some((
                    value["project"].as_str()?.to_owned(),
                    current_trust(&value, value["trust"].as_str()?, keyring),
                    value["counts"]["articles"].as_u64()?,
                ))
            });
        listed.push(match parsed {
            Some((project, trust, articles)) => InstalledSnapshotInfo {
                snapshot_id: id,
                project,
                trust,
                articles,
                damaged: false,
            },
            None => InstalledSnapshotInfo {
                snapshot_id: id,
                project: String::new(),
                trust: TrustStatus::VerificationFailed,
                articles: 0,
                damaged: true,
            },
        });
    }
    listed.sort_by(|a, b| a.snapshot_id.cmp(&b.snapshot_id));
    Ok(listed)
}

/// Remove an installed, inactive snapshot. The directory is first renamed
/// into the private staging area (atomic on one filesystem) and both
/// directories are synced, so a crash can only leave `remove-*` residue in
/// staging — never a partially deleted snapshot that still looks installed.
/// The active snapshot is refused. The caller must own `root` exclusively.
pub fn remove_installed_snapshot(root: &Path, snapshot_id: &str) -> Result<(), PackInstallError> {
    remove_installed_snapshot_with_sync(root, snapshot_id, sync_dir)
}

fn remove_installed_snapshot_with_sync(
    root: &Path,
    snapshot_id: &str,
    mut sync: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), PackInstallError> {
    if !safe_id(snapshot_id) {
        return Err(PackInstallError::InvalidSnapshotId);
    }
    let (snapshots, staging) = prepare_root(root)?;
    let target = snapshots.join(snapshot_id);
    if !real_directory(&target)? {
        return Err(PackInstallError::NotInstalled);
    }
    let active = crate::activation::ActiveSnapshotStore::new(root)
        .current()
        .map_err(|_| PackInstallError::UnsafeRoot)?;
    if active.as_deref() == Some(snapshot_id) {
        return Err(PackInstallError::SnapshotActive);
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let doomed = staging.join(format!(
        "remove-{}-{nonce}-{}",
        std::process::id(),
        NEXT_STAGE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::rename(&target, &doomed)?;
    sync(&snapshots)?;
    sync(&staging)?;
    fs::remove_dir_all(&doomed)?;
    sync(&staging)?;
    Ok(())
}

/// What a search index repair found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchIndexRepair {
    /// The existing index already verified; nothing was changed.
    AlreadyValid,
    /// The index was missing or failed verification and was rebuilt from
    /// the snapshot's verified shards.
    Rebuilt,
}

/// STORE-004: repair an installed snapshot's derived search index without
/// any download. The catalog must still match the digest the installer
/// recorded and every shard must rehash to the installed manifest; only
/// then is `search.sqlite` regenerated (atomically, and verified against
/// the catalog before it replaces the old file). Damaged canonical content
/// (catalog or shards) is refused: it can only be fixed by reinstalling.
/// The caller must not hold the snapshot open while repairing it.
pub fn repair_installed_search_index(
    root: &Path,
    snapshot_id: &str,
) -> Result<SearchIndexRepair, PackInstallError> {
    if !safe_id(snapshot_id) {
        return Err(PackInstallError::InvalidSnapshotId);
    }
    let (snapshots, _) = prepare_root(root)?;
    let snapshot = snapshots.join(snapshot_id);
    if !real_directory(&snapshot)? {
        return Err(PackInstallError::NotInstalled);
    }
    let catalog_path = snapshot.join(CATALOG_OBJECT);
    let search_path = snapshot.join(SEARCH_OBJECT);
    let shards = installed_shard_inventory(&snapshot)?;
    if VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &catalog_path,
        &search_path,
        &snapshot,
        &shards,
    )
    .is_ok()
    {
        return Ok(SearchIndexRepair::AlreadyValid);
    }
    verify_recorded_catalog(&snapshot)?;
    let catalog = SnapshotCatalog::open_read_only(&catalog_path)
        .map_err(|error| PackInstallError::Reader(ReaderError::Catalog(error)))?;
    OfflineSearchIndex::replace_persisted_from_verified_shards(
        &search_path,
        &catalog,
        &snapshot,
        &shards,
    )
    .map_err(PackInstallError::Search)?;
    drop(catalog);
    VerifiedOfflineCatalogReader::open_with_manifest_shards(
        &catalog_path,
        &search_path,
        &snapshot,
        &shards,
    )
    .map_err(PackInstallError::Reader)?;
    Ok(SearchIndexRepair::Rebuilt)
}

/// The catalog file must still be byte-identical to the one installed.
fn verify_recorded_catalog(snapshot: &Path) -> Result<(), PackInstallError> {
    use sha2::{Digest, Sha256};
    let invalid = || PackInstallError::UnsupportedObject(MANIFEST_FILE.into());
    let path = snapshot.join(MANIFEST_FILE);
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() > MAX_INSTALLED_MANIFEST_BYTES {
        return Err(invalid());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&path)?).map_err(|_| invalid())?;
    let recorded = value["objects"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .find(|object| object["path"].as_str() == Some(CATALOG_OBJECT))
        .ok_or_else(invalid)?;
    let (Some(sha256), Some(bytes)) = (recorded["sha256"].as_str(), recorded["bytes"].as_u64())
    else {
        return Err(invalid());
    };
    let catalog = snapshot.join(CATALOG_OBJECT);
    let meta = fs::symlink_metadata(&catalog)?;
    let damaged = || {
        PackInstallError::Reader(ReaderError::Catalog(
            crate::catalog::CatalogError::DigestMismatch,
        ))
    };
    if !meta.is_file() || meta.len() != bytes {
        return Err(damaged());
    }
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(&catalog)?, &mut hasher)?;
    if !signature::hex(&hasher.finalize()[..]).eq_ignore_ascii_case(sha256) {
        return Err(damaged());
    }
    Ok(())
}

/// The text licence (an SPDX identifier) the installer recorded for an
/// installed snapshot, for the attribution section (SEC-002). `None` when
/// the record is missing, too large, unreadable or the identifier is not
/// SPDX-shaped: the reader then says no licence is stated rather than
/// guessing one.
pub fn installed_text_license(snapshot: &Path) -> Option<String> {
    let path = snapshot.join(MANIFEST_FILE);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_INSTALLED_MANIFEST_BYTES {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let spdx = value["license"]["text_spdx"].as_str()?;
    wiki_pack_format::manifest::valid_spdx(spdx).then(|| spdx.to_owned())
}

/// Reopen an installed snapshot's pinned shard inventory from the
/// installer-owned `manifest.json`, for verified opening after a restart.
/// Index files and the attribution object are excluded exactly as at install
/// time. The manifest is bounded, must be a regular file, and every entry is
/// revalidated; the returned inventory is then rehashed by the verified
/// reader, so a tampered manifest cannot make altered shards verify.
pub fn installed_shard_inventory(snapshot: &Path) -> Result<ShardInventory, PackInstallError> {
    let path = snapshot.join(MANIFEST_FILE);
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() > MAX_INSTALLED_MANIFEST_BYTES {
        return Err(PackInstallError::UnsupportedObject(MANIFEST_FILE.into()));
    }
    let invalid = || PackInstallError::UnsupportedObject(MANIFEST_FILE.into());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&path)?).map_err(|_| invalid())?;
    let mut skip: Vec<&str> = value["indexes"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|index| index["path"].as_str().ok_or_else(invalid))
        .collect::<Result<_, _>>()?;
    if let Some(attribution) = value["license"]["attribution_object"].as_str() {
        skip.push(attribution);
    }
    let mut shards = ShardInventory::new();
    for object in value["objects"].as_array().ok_or_else(invalid)? {
        let path = object["path"].as_str().ok_or_else(invalid)?;
        let sha256 = object["sha256"].as_str().ok_or_else(invalid)?;
        let bytes = object["bytes"].as_u64().ok_or_else(invalid)?;
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        if skip.contains(&path) || is_media_object(path) {
            continue;
        }
        if shards
            .insert(path.to_owned(), (bytes, sha256.to_ascii_lowercase()))
            .is_some()
        {
            return Err(invalid());
        }
    }
    Ok(shards)
}

/// The installer, not the pack, owns `manifest.json`: it records the verified
/// envelope identity and object inventory the snapshot was installed from.
fn write_snapshot_manifest(
    staged: &Path,
    manifest: &Manifest,
    trust: TrustStatus,
    signing_key: Option<[u8; 8]>,
) -> io::Result<()> {
    let origin = match &manifest.origin {
        Origin::Custom { definition_id } => {
            serde_json::json!({ "custom": { "definition_id": definition_id } })
        }
        Origin::Official { publisher } => serde_json::json!({ "official": {
            "publisher": publisher,
            "key_id": signing_key.map(|id| signature::hex(&id)),
        } }),
    };
    let objects: Vec<_> = manifest
        .objects
        .iter()
        .map(|object| {
            serde_json::json!({
                "path": object.path,
                "sha256": object.sha256,
                "bytes": object.bytes,
            })
        })
        .collect();
    let document = serde_json::json!({
        "format_version": manifest.version,
        "pack_id": manifest.pack_id,
        "project": manifest.project,
        "snapshot": manifest.snapshot,
        "origin": origin,
        "trust": trust.label(),
        "source": {
            "kind": manifest.metadata.source.kind.name(),
            "snapshot_id": manifest.metadata.source.snapshot_id,
            "manifest_sha256": manifest.metadata.source.manifest_sha256,
        },
        "definition_sha256": manifest.metadata.definition_sha256,
        "license": {
            "text_spdx": manifest.metadata.license.text_spdx,
            "attribution_object": manifest.metadata.license.attribution_object,
        },
        "indexes": manifest.metadata.indexes.iter().map(|index| serde_json::json!({
            "path": index.path,
            "role": index.role.name(),
            "schema": index.schema,
        })).collect::<Vec<_>>(),
        "counts": {
            "articles": manifest.metadata.counts.articles,
            "redirects": manifest.metadata.counts.redirects,
            "media": manifest.metadata.counts.media,
        },
        "objects": objects,
    });
    let mut bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(staged.join(MANIFEST_FILE))?;
    file.write_all(&bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activation::ActiveSnapshotStore;
    use crate::record_codec::append_record_frame;
    use std::io::Cursor;
    use wiki_model::{
        Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
    };
    use wiki_pack_format::container::write_container;
    use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};

    struct TestDir(PathBuf);

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_dir(label: &str) -> TestDir {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "wiki-pack-install-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        TestDir(dir)
    }

    fn key() -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id: 11,
        }
    }

    /// Build a snapshot-layout tree (catalog, one shard, optional index).
    fn snapshot_tree(dir: &Path, revision_id: u64, with_index: bool) {
        fs::create_dir_all(dir).unwrap();
        let article = Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: key(),
            revision: Revision {
                revision_id,
                timestamp: "2026-11-01T00:00:00Z".into(),
                content_sha256: "c".repeat(64),
            },
            title: "Mars".into(),
            display_title: "Mars".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("the red planet".into()),
            }],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Mars</article>".into(),
            is_disambiguation: false,
        };
        let mut shard = Vec::new();
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article))).unwrap();
        fs::write(dir.join("articles.shard"), &shard).unwrap();
        let mut catalog = SnapshotCatalog::open(&dir.join(CATALOG_OBJECT)).unwrap();
        catalog
            .insert_verified(dir, "articles.shard", 0, shard.len() as u64)
            .unwrap();
        if with_index {
            OfflineSearchIndex::replace_persisted_from_catalog(
                &dir.join(SEARCH_OBJECT),
                &catalog,
                dir,
            )
            .unwrap();
        }
    }

    /// Metadata derived from the tree, or, for deliberately broken trees
    /// the derivation cannot scan, the declared indexes with zero counts.
    fn metadata_for(tree: &Path) -> PackMetadata {
        let fixture = PackMetadata::synthetic_fixture();
        snapshot_pack_metadata(
            tree,
            fixture.source.clone(),
            &"0".repeat(64),
            "CC-BY-SA-4.0",
        )
        .unwrap_or_else(|_| {
            let mut metadata = fixture;
            for (path, role, schema) in [
                (CATALOG_OBJECT, IndexRole::Catalog, CATALOG_SCHEMA),
                (SEARCH_OBJECT, IndexRole::Fts5, FTS5_SCHEMA),
            ] {
                if tree.join(path).is_file() {
                    metadata.indexes.push(IndexEntry {
                        path: path.into(),
                        role,
                        schema,
                    });
                }
            }
            metadata
        })
    }

    fn pack_with(tree: &Path, pack_id: &str, metadata: PackMetadata) -> Vec<u8> {
        let manifest =
            custom_manifest_from_tree(tree, pack_id, "enwiki", "20261101", "mars-def", metadata)
                .unwrap();
        let mut bytes = Vec::new();
        write_custom_tree(tree, &manifest, &mut bytes).unwrap();
        bytes
    }

    fn pack_bytes(tree: &Path, pack_id: &str) -> Vec<u8> {
        pack_with(tree, pack_id, metadata_for(tree))
    }

    fn install(root: &Path, bytes: &[u8]) -> Result<InstalledPack, PackInstallError> {
        install_custom_pack(root, &mut Cursor::new(bytes), 1 << 30, |_| true)
    }

    fn staging_entries(root: &Path) -> usize {
        fs::read_dir(root.join("staging")).unwrap().count()
    }

    fn open_revision(installed: &InstalledPack) -> u64 {
        let reader = VerifiedOfflineCatalogReader::open_with_manifest_shards(
            &installed.path.join(CATALOG_OBJECT),
            &installed.path.join(SEARCH_OBJECT),
            &installed.path,
            &installed.shards,
        )
        .unwrap();
        let page = reader.reader().open_page(&key()).unwrap().unwrap();
        page.revision.revision_id
    }

    #[test]
    fn verified_custom_pack_installs_inactive_then_activates() {
        let dir = test_dir("happy");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3001, true);
        let bytes = pack_bytes(&tree, "mars-pack");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();

        let mut reviewed = None;
        let installed = install_custom_pack(&root, &mut Cursor::new(&bytes), 1 << 30, |review| {
            reviewed = Some((
                review.manifest.pack_id.clone(),
                review.trust,
                review.declared_bytes,
                review.rebuilt_search_index,
            ));
            true
        })
        .unwrap();
        let (pack_id, trust, declared, rebuilt) = reviewed.unwrap();
        assert_eq!(pack_id, "mars-pack");
        assert_eq!(trust, TrustStatus::CustomIntegrityVerified);
        assert!(declared > 0);
        assert!(!rebuilt);
        assert_eq!(installed.snapshot_id, "mars-pack");
        assert_eq!(installed.path, root.join("snapshots/mars-pack"));
        assert_eq!(installed.trust, TrustStatus::CustomIntegrityVerified);
        assert_eq!(
            installed.shards.keys().collect::<Vec<_>>(),
            ["articles.shard"]
        );
        assert_eq!(staging_entries(&root), 0);

        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(installed.path.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(manifest["pack_id"], "mars-pack");
        assert_eq!(manifest["trust"], "Custom — integrity verified");
        assert_eq!(manifest["objects"].as_array().unwrap().len(), 3);

        // Installing never selects the snapshot; activation is separate.
        let store = ActiveSnapshotStore::new(&root);
        assert_eq!(store.current().unwrap(), None);
        store
            .switch_to_prevalidated(&installed.snapshot_id)
            .unwrap();
        assert_eq!(store.current().unwrap().as_deref(), Some("mars-pack"));
        assert_eq!(open_revision(&installed), 3001);
    }

    #[test]
    fn omitted_search_index_is_rebuilt_from_verified_shards() {
        let dir = test_dir("rebuild");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3002, false);
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let mut rebuilt = false;
        let installed = install_custom_pack(
            &root,
            &mut Cursor::new(pack_bytes(&tree, "mars-noindex")),
            1 << 30,
            |review| {
                rebuilt = review.rebuilt_search_index;
                true
            },
        )
        .unwrap();
        assert!(rebuilt);
        assert_eq!(open_revision(&installed), 3002);
        let reader = VerifiedOfflineCatalogReader::open_with_manifest_shards(
            &installed.path.join(CATALOG_OBJECT),
            &installed.path.join(SEARCH_OBJECT),
            &installed.path,
            &installed.shards,
        )
        .unwrap();
        assert_eq!(
            reader.reader().search_articles("planet", 5).unwrap().len(),
            1
        );
    }

    #[test]
    fn declined_confirmation_leaves_no_snapshot_and_keeps_selector() {
        let dir = test_dir("declined");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let old_tree = dir.0.join("old");
        snapshot_tree(&old_tree, 3003, true);
        install(&root, &pack_bytes(&old_tree, "old")).unwrap();
        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("old").unwrap();

        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3004, true);
        let result = install_custom_pack(
            &root,
            &mut Cursor::new(pack_bytes(&tree, "new")),
            1 << 30,
            |_| false,
        );
        assert!(matches!(result, Err(PackInstallError::Declined)));
        assert!(!root.join("snapshots/new").exists());
        assert_eq!(staging_entries(&root), 0);
        assert_eq!(store.current().unwrap().as_deref(), Some("old"));
    }

    #[test]
    fn unsigned_official_claim_is_refused_before_confirmation() {
        let dir = test_dir("official");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3005, true);
        let mut manifest = custom_manifest_from_tree(
            &tree,
            "official-pack",
            "enwiki",
            "20261101",
            "x",
            metadata_for(&tree),
        )
        .unwrap();
        manifest.origin = Origin::Official {
            publisher: "Wikimedia".into(),
        };
        let mut bytes = Vec::new();
        write_container(&mut bytes, &manifest, |object, writer| {
            writer.write_all(&fs::read(tree.join(&object.path))?)
        })
        .unwrap();
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let result = install_custom_pack(&root, &mut Cursor::new(bytes), 1 << 30, |_| {
            panic!("an unsigned official pack must never reach confirmation")
        });
        assert!(matches!(
            result,
            Err(PackInstallError::UnsignedOfficialPack)
        ));
        assert!(!root.join("snapshots/official-pack").exists());
        assert_eq!(staging_entries(&root), 0);
    }

    /// An official pack (snapshot 20261101) and its bytes, for signing tests.
    fn official_pack(tree: &Path, pack_id: &str) -> (Manifest, Vec<u8>) {
        let mut manifest =
            custom_manifest_from_tree(tree, pack_id, "enwiki", "20261101", "x", metadata_for(tree))
                .unwrap();
        manifest.origin = Origin::Official {
            publisher: "foundation".into(),
        };
        let mut bytes = Vec::new();
        write_container(&mut bytes, &manifest, |object, writer| {
            writer.write_all(&fs::read(tree.join(&object.path))?)
        })
        .unwrap();
        (manifest, bytes)
    }

    /// A throwaway publisher key, never pinned in the production keyring.
    const TEST_SECRET: [u8; 32] = [7; 32];

    fn test_keyring() -> Vec<TrustedKey> {
        vec![TrustedKey {
            publisher: "foundation",
            public_key: signature::public_key(&TEST_SECRET),
            valid_from: "20260101",
            valid_until: None,
            revoked: false,
        }]
    }

    #[test]
    fn a_signed_official_pack_installs_as_verified_publisher_until_its_key_is_revoked() {
        let dir = test_dir("signed");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3006, true);
        let (manifest, bytes) = official_pack(&tree, "signed-pack");
        let signed = signature::sign(&manifest, &TEST_SECRET).unwrap();
        let keyring = test_keyring();
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let mut reviewed = None;
        let installed = install_official_pack(
            &root,
            &mut Cursor::new(bytes.as_slice()),
            1 << 30,
            PublisherProof {
                signature: &signed,
                keyring: &keyring,
            },
            |review| {
                reviewed = Some(review.trust);
                true
            },
        )
        .unwrap();
        assert_eq!(reviewed, Some(TrustStatus::VerifiedPublisher));
        assert_eq!(installed.trust, TrustStatus::VerifiedPublisher);
        assert_eq!(open_revision(&installed), 3006);
        let recorded: serde_json::Value =
            serde_json::from_slice(&fs::read(installed.path.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(recorded["origin"]["official"]["publisher"], "foundation");
        assert_eq!(recorded["trust"], "Verified publisher");

        let label = |keyring: &[TrustedKey]| {
            list_installed_snapshots_with_keyring(&root, keyring).unwrap()[0]
                .trust
                .label()
        };
        assert_eq!(label(&keyring), "Verified publisher");
        // Revoking (or no longer pinning) the key withdraws the label.
        let revoked = vec![TrustedKey {
            revoked: true,
            ..keyring[0].clone()
        }];
        assert_eq!(label(&revoked), "Verification failed");
        assert_eq!(label(&[]), "Verification failed");
        assert_eq!(
            list_installed_snapshots(&root).unwrap()[0].trust.label(),
            "Verification failed",
            "the throwaway test key is not in the production keyring"
        );
    }

    #[test]
    fn a_tampered_or_wrongly_signed_official_pack_never_installs() {
        let dir = test_dir("forged");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3007, true);
        let (manifest, bytes) = official_pack(&tree, "forged-pack");
        let signed = signature::sign(&manifest, &TEST_SECRET).unwrap();
        let keyring = test_keyring();
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let install = |bytes: &[u8], proof: PublisherProof<'_>| {
            install_official_pack(&root, &mut Cursor::new(bytes), 1 << 30, proof, |_| {
                panic!("a forged pack must never reach confirmation")
            })
        };
        let proof = PublisherProof {
            signature: &signed,
            keyring: &keyring,
        };
        // One flipped payload bit: the bytes no longer match the signed manifest.
        let mut tampered = bytes.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(matches!(
            install(&tampered, proof),
            Err(PackInstallError::Container(_))
        ));
        // A forged signature, an unpinned key, and a signature of another pack.
        let mut forged = signed.clone();
        forged.signature[10] ^= 1;
        let other = signature::sign(&manifest, &[9; 32]).unwrap();
        let (other_manifest, _) = official_pack(&tree, "other-pack");
        let elsewhere = signature::sign(&other_manifest, &TEST_SECRET).unwrap();
        for (bad, why) in [
            (&forged, SignatureError::BadSignature),
            (&other, SignatureError::UnknownKey),
            (&elsewhere, SignatureError::TranscriptMismatch),
        ] {
            let result = install(
                &bytes,
                PublisherProof {
                    signature: bad,
                    keyring: &keyring,
                },
            );
            assert!(
                matches!(&result, Err(PackInstallError::PublisherVerificationFailed(e)) if *e == why),
                "{result:?}"
            );
        }
        assert!(!root.join("snapshots/forged-pack").exists());
        assert_eq!(staging_entries(&root), 0);

        // A custom pack cannot be installed through the official path.
        let custom = pack_bytes(&tree, "custom-pack");
        assert!(matches!(
            install(&custom, proof),
            Err(PackInstallError::NotOfficialPack)
        ));
        assert_eq!(staging_entries(&root), 0);
    }

    #[test]
    fn catalog_shard_mismatch_is_refused_and_rolled_back() {
        let dir = test_dir("mismatch");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3006, true);
        // The envelope digests stay self-consistent, but the catalog no
        // longer describes the shard bytes.
        fs::write(tree.join("articles.shard"), b"not the indexed frames").unwrap();
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let result = install_custom_pack(
            &root,
            &mut Cursor::new(pack_bytes(&tree, "broken")),
            1 << 30,
            |_| panic!("an unverifiable pack must never reach confirmation"),
        );
        assert!(matches!(result, Err(PackInstallError::Reader(_))));
        assert!(!root.join("snapshots/broken").exists());
        assert_eq!(staging_entries(&root), 0);
    }

    #[test]
    fn layout_violations_are_refused() {
        let dir = test_dir("layout");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();

        let reserved = dir.0.join("reserved");
        snapshot_tree(&reserved, 3007, true);
        fs::write(reserved.join(MANIFEST_FILE), b"{}").unwrap();
        assert!(matches!(
            install(&root, &pack_bytes(&reserved, "reserved")),
            Err(PackInstallError::UnsupportedObject(path)) if path == MANIFEST_FILE
        ));

        let sidecar = dir.0.join("sidecar");
        snapshot_tree(&sidecar, 3008, true);
        fs::write(sidecar.join("catalog.sqlite-wal"), b"wal").unwrap();
        assert!(matches!(
            install(&root, &pack_bytes(&sidecar, "sidecar")),
            Err(PackInstallError::UnsupportedObject(_))
        ));

        let extra = dir.0.join("extra");
        snapshot_tree(&extra, 3009, true);
        fs::write(extra.join("unlisted.shard"), b"bytes no catalog names").unwrap();
        assert!(matches!(
            install(&root, &pack_bytes(&extra, "extra")),
            Err(PackInstallError::Reader(_))
        ));

        let no_catalog = dir.0.join("no-catalog");
        fs::create_dir_all(&no_catalog).unwrap();
        fs::write(no_catalog.join("articles.shard"), b"orphan").unwrap();
        assert!(matches!(
            install(&root, &pack_bytes(&no_catalog, "no-catalog")),
            Err(PackInstallError::MissingCatalog)
        ));

        let bad_id = dir.0.join("bad-id");
        snapshot_tree(&bad_id, 3010, true);
        assert!(matches!(
            install(&root, &pack_bytes(&bad_id, "Mars")),
            Err(PackInstallError::InvalidSnapshotId)
        ));

        assert_eq!(fs::read_dir(root.join("snapshots")).unwrap().count(), 0);
        assert_eq!(staging_entries(&root), 0);
    }

    #[test]
    fn existing_snapshot_is_never_replaced() {
        let dir = test_dir("existing");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let first = dir.0.join("first");
        snapshot_tree(&first, 3011, true);
        let installed = install(&root, &pack_bytes(&first, "mars")).unwrap();
        let before = fs::read(installed.path.join("articles.shard")).unwrap();

        let second = dir.0.join("second");
        snapshot_tree(&second, 3012, true);
        assert!(matches!(
            install(&root, &pack_bytes(&second, "mars")),
            Err(PackInstallError::AlreadyInstalled)
        ));
        assert_eq!(
            fs::read(installed.path.join("articles.shard")).unwrap(),
            before
        );
        assert_eq!(open_revision(&installed), 3011);
        assert_eq!(staging_entries(&root), 0);
    }

    #[test]
    fn stream_install_matches_seekable_install() {
        let dir = test_dir("stream");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3013, true);
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let bytes = pack_bytes(&tree, "mars-stream");
        let installed =
            install_custom_pack_stream(&root, &mut bytes.as_slice(), 1 << 30, |_| true).unwrap();
        assert_eq!(open_revision(&installed), 3013);
        assert_eq!(staging_entries(&root), 0);
    }

    #[test]
    fn reclaim_removes_only_install_residue() {
        let dir = test_dir("reclaim");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3015, true);
        let installed = install(&root, &pack_bytes(&tree, "kept")).unwrap();

        let staging = root.join("staging");
        fs::create_dir_all(staging.join("install-1-2-3/nested")).unwrap();
        fs::write(staging.join("install-1-2-3/nested/articles.shard"), b"x").unwrap();
        fs::write(staging.join(".wpack-spool-1-2-3"), b"partial").unwrap();
        fs::write(staging.join("operator-note"), b"not ours").unwrap();
        #[cfg(unix)]
        {
            let outside = dir.0.join("outside");
            fs::create_dir_all(&outside).unwrap();
            fs::write(outside.join("keep"), b"keep").unwrap();
            std::os::unix::fs::symlink(&outside, staging.join("install-link")).unwrap();
        }

        let (removed, retained) = reclaim_abandoned_staging(&root).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(retained, if cfg!(unix) { 2 } else { 1 });
        assert!(!staging.join("install-1-2-3").exists());
        assert!(!staging.join(".wpack-spool-1-2-3").exists());
        assert!(staging.join("operator-note").exists());
        #[cfg(unix)]
        assert_eq!(fs::read(dir.0.join("outside/keep")).unwrap(), b"keep");
        // Published snapshots are never touched.
        assert_eq!(open_revision(&installed), 3015);
        assert_eq!(reclaim_abandoned_staging(&root).unwrap().0, 0);
    }

    #[test]
    fn manifest_metadata_is_checked_against_the_snapshot() {
        let dir = test_dir("metadata");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3016, true);
        let derived = metadata_for(&tree);
        assert_eq!(derived.counts.articles, 1);
        assert_eq!(derived.indexes.len(), 2);

        let mut wrong_count = derived.clone();
        wrong_count.counts.articles = 2;
        assert!(matches!(
            install(&root, &pack_with(&tree, "wrong-count", wrong_count)),
            Err(PackInstallError::CountMismatch { declared, actual })
                if declared.articles == 2 && actual.articles == 1
        ));

        let mut wrong_redirects = derived.clone();
        wrong_redirects.counts.redirects = 1;
        assert!(matches!(
            install(&root, &pack_with(&tree, "wrong-redirects", wrong_redirects)),
            Err(PackInstallError::CountMismatch { .. })
        ));

        // A search index that is present but not declared is not content.
        let mut undeclared = derived.clone();
        undeclared
            .indexes
            .retain(|index| index.role != IndexRole::Fts5);
        assert!(matches!(
            install(&root, &pack_with(&tree, "undeclared", undeclared)),
            Err(PackInstallError::UnsupportedObject(path)) if path == SEARCH_OBJECT
        ));

        let mut future_schema = derived.clone();
        future_schema.indexes[0].schema = CATALOG_SCHEMA + 1;
        assert!(matches!(
            install(&root, &pack_with(&tree, "future-schema", future_schema)),
            Err(PackInstallError::UnsupportedObject(_))
        ));

        let mut no_catalog_role = derived.clone();
        no_catalog_role
            .indexes
            .retain(|index| index.role != IndexRole::Catalog);
        assert!(matches!(
            install(&root, &pack_with(&tree, "no-catalog-role", no_catalog_role)),
            Err(PackInstallError::MissingCatalog)
        ));

        // Declared media need the canonical image index (PACK-003).
        let with_attribution = dir.0.join("attributed");
        snapshot_tree(&with_attribution, 3017, true);
        fs::write(with_attribution.join("attribution.json"), b"[]").unwrap();
        let mut media = metadata_for(&with_attribution);
        media.license.attribution_object = Some("attribution.json".into());
        media.counts.media = 1;
        assert!(matches!(
            install(&root, &pack_with(&with_attribution, "media", media.clone())),
            Err(PackInstallError::Media(PackMediaError::UndeclaredMedia))
        ));

        assert_eq!(fs::read_dir(root.join("snapshots")).unwrap().count(), 0);
        assert_eq!(staging_entries(&root), 0);

        // A text-only pack may still carry an attribution object; it is kept
        // in the snapshot and is not treated as an article shard.
        media.counts.media = 0;
        let installed = install(&root, &pack_with(&with_attribution, "attributed", media)).unwrap();
        assert!(installed.path.join("attribution.json").is_file());
        assert!(!installed.shards.contains_key("attribution.json"));
        assert_eq!(open_revision(&installed), 3017);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(installed.path.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(manifest["format_version"], 2);
        assert_eq!(manifest["counts"]["articles"], 1);
        assert_eq!(
            manifest["license"]["attribution_object"],
            "attribution.json"
        );
        assert_eq!(manifest["indexes"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn the_recorded_text_licence_is_read_back_and_checked() {
        let dir = test_dir("text-licence");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3019, true);
        let mut metadata = metadata_for(&tree);
        metadata.license.text_spdx = "CC-BY-SA-3.0".into();
        let installed = install(&root, &pack_with(&tree, "licence", metadata)).unwrap();
        assert_eq!(
            installed_text_license(&installed.path).as_deref(),
            Some("CC-BY-SA-3.0")
        );

        let manifest = installed.path.join(MANIFEST_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["license"]["text_spdx"] = "<b>free</b>".into();
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(installed_text_license(&installed.path), None);
        value["license"] = serde_json::Value::Null;
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(installed_text_license(&installed.path), None);
        fs::remove_file(&manifest).unwrap();
        assert_eq!(installed_text_license(&installed.path), None);
    }

    #[test]
    fn installed_inventory_reopens_the_verified_snapshot() {
        let dir = test_dir("inventory");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3018, true);
        fs::write(tree.join("attribution.json"), b"[]").unwrap();
        let mut metadata = metadata_for(&tree);
        metadata.license.attribution_object = Some("attribution.json".into());
        let installed = install(&root, &pack_with(&tree, "reopen", metadata)).unwrap();
        let inventory = installed_shard_inventory(&installed.path).unwrap();
        assert_eq!(inventory, installed.shards);
        assert_eq!(open_revision(&installed), 3018);

        // A tampered manifest digest cannot make the shard verify.
        let manifest_path = installed.path.join(MANIFEST_FILE);
        let original = fs::read_to_string(&manifest_path).unwrap();
        let shard_digest = &installed.shards["articles.shard"].1;
        fs::write(
            &manifest_path,
            original.replace(shard_digest.as_str(), &"0".repeat(64)),
        )
        .unwrap();
        let forged = installed_shard_inventory(&installed.path).unwrap();
        assert!(VerifiedOfflineCatalogReader::open_with_manifest_shards(
            &installed.path.join(CATALOG_OBJECT),
            &installed.path.join(SEARCH_OBJECT),
            &installed.path,
            &forged,
        )
        .is_err());
        fs::write(&manifest_path, "{not json").unwrap();
        assert!(installed_shard_inventory(&installed.path).is_err());
    }

    #[test]
    fn listing_and_safe_removal_of_installed_snapshots() {
        let dir = test_dir("remove");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        for (id, revision) in [("alpha", 3020), ("beta", 3021)] {
            let tree = dir.0.join(id);
            snapshot_tree(&tree, revision, true);
            install(&root, &pack_bytes(&tree, id)).unwrap();
        }
        let listed = list_installed_snapshots(&root).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|s| s.snapshot_id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        assert_eq!(listed[0].trust, TrustStatus::CustomIntegrityVerified);
        assert_eq!((listed[0].articles, listed[0].damaged), (1, false));

        let store = ActiveSnapshotStore::new(&root);
        store.switch_to_prevalidated("alpha").unwrap();
        assert!(matches!(
            remove_installed_snapshot(&root, "alpha"),
            Err(PackInstallError::SnapshotActive)
        ));
        assert!(matches!(
            remove_installed_snapshot(&root, "gamma"),
            Err(PackInstallError::NotInstalled)
        ));
        assert!(matches!(
            remove_installed_snapshot(&root, "../alpha"),
            Err(PackInstallError::InvalidSnapshotId)
        ));
        remove_installed_snapshot(&root, "beta").unwrap();
        assert!(!root.join("snapshots/beta").exists());
        assert_eq!(staging_entries(&root), 0);
        assert_eq!(store.current().unwrap().as_deref(), Some("alpha"));
        assert_eq!(list_installed_snapshots(&root).unwrap().len(), 1);

        // A damaged snapshot is listed, not hidden, and can be removed.
        let tree = dir.0.join("gamma");
        snapshot_tree(&tree, 3022, true);
        install(&root, &pack_bytes(&tree, "gamma")).unwrap();
        fs::write(root.join("snapshots/gamma").join(MANIFEST_FILE), "{").unwrap();
        let gamma = list_installed_snapshots(&root)
            .unwrap()
            .into_iter()
            .find(|s| s.snapshot_id == "gamma")
            .unwrap();
        assert!(gamma.damaged);
        assert_eq!(gamma.trust, TrustStatus::VerificationFailed);
        remove_installed_snapshot(&root, "gamma").unwrap();
    }

    #[test]
    fn a_damaged_or_missing_search_index_is_rebuilt_from_verified_shards() {
        let dir = test_dir("repair-index");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("epsilon");
        snapshot_tree(&tree, 3030, true);
        let installed = install(&root, &pack_bytes(&tree, "epsilon")).unwrap();
        let search = installed.path.join(SEARCH_OBJECT);
        let open = || {
            VerifiedOfflineCatalogReader::open_with_manifest_shards(
                &installed.path.join(CATALOG_OBJECT),
                &search,
                &installed.path,
                &installed_shard_inventory(&installed.path).unwrap(),
            )
        };
        let titles = || {
            let reader = open().unwrap();
            reader
                .reader()
                .search_articles("planet", 10)
                .unwrap()
                .into_iter()
                .map(|article| article.title)
                .collect::<Vec<_>>()
        };
        let expected = titles();
        assert!(!expected.is_empty());
        assert_eq!(
            repair_installed_search_index(&root, "epsilon").unwrap(),
            SearchIndexRepair::AlreadyValid
        );

        let shard_bytes = |path: &Path| {
            let mut files: Vec<_> = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "shard"))
                .map(|path| fs::read(path).unwrap())
                .collect();
            files.sort();
            files
        };
        let shards_before = shard_bytes(&installed.path);
        let catalog_before = fs::read(installed.path.join(CATALOG_OBJECT)).unwrap();

        for damage in ["garbage", "truncate", "missing"] {
            match damage {
                "garbage" => fs::write(&search, b"not a database").unwrap(),
                "truncate" => {
                    let bytes = fs::read(&search).unwrap();
                    fs::write(&search, &bytes[..bytes.len() / 2]).unwrap();
                }
                _ => fs::remove_file(&search).unwrap(),
            }
            assert!(open().is_err(), "{damage}: damaged index must not open");
            assert_eq!(
                repair_installed_search_index(&root, "epsilon").unwrap(),
                SearchIndexRepair::Rebuilt,
                "{damage}"
            );
            assert_eq!(titles(), expected, "{damage}");
        }
        // Canonical content was only read, never rewritten.
        assert_eq!(shard_bytes(&installed.path), shards_before);
        assert_eq!(
            fs::read(installed.path.join(CATALOG_OBJECT)).unwrap(),
            catalog_before
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&search).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        assert!(!fs::read_dir(&installed.path).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".offline-fts.")));
    }

    #[test]
    fn damaged_canonical_content_is_never_repaired_by_a_rebuild() {
        let dir = test_dir("repair-refuse");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("zeta");
        snapshot_tree(&tree, 3031, true);
        let installed = install(&root, &pack_bytes(&tree, "zeta")).unwrap();
        let search = installed.path.join(SEARCH_OBJECT);
        fs::write(&search, b"broken").unwrap();

        // A changed shard byte: the index stays broken; reinstall is needed.
        let shard = fs::read_dir(&installed.path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "shard"))
            .unwrap();
        let original = fs::read(&shard).unwrap();
        let mut changed = original.clone();
        *changed.last_mut().unwrap() ^= 1;
        fs::write(&shard, &changed).unwrap();
        assert!(repair_installed_search_index(&root, "zeta").is_err());
        assert_eq!(fs::read(&search).unwrap(), b"broken");
        fs::write(&shard, &original).unwrap();

        // A changed catalog is refused before anything is rebuilt.
        let catalog = installed.path.join(CATALOG_OBJECT);
        let original = fs::read(&catalog).unwrap();
        let mut changed = original.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        fs::write(&catalog, &changed).unwrap();
        assert!(matches!(
            repair_installed_search_index(&root, "zeta"),
            Err(PackInstallError::Reader(ReaderError::Catalog(
                crate::catalog::CatalogError::DigestMismatch
            )))
        ));
        assert_eq!(fs::read(&search).unwrap(), b"broken");
        fs::write(&catalog, &original).unwrap();
        assert_eq!(
            repair_installed_search_index(&root, "zeta").unwrap(),
            SearchIndexRepair::Rebuilt
        );

        assert!(matches!(
            repair_installed_search_index(&root, "missing"),
            Err(PackInstallError::NotInstalled)
        ));
        assert!(matches!(
            repair_installed_search_index(&root, "../zeta"),
            Err(PackInstallError::InvalidSnapshotId)
        ));
    }

    #[test]
    fn altered_installed_trust_records_are_shown_as_failed() {
        let dir = test_dir("altered-trust");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("delta");
        snapshot_tree(&tree, 3023, true);
        let installed = install(&root, &pack_bytes(&tree, "delta")).unwrap();
        let manifest_path = installed.path.join(MANIFEST_FILE);
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        // Promoting a custom pack, or any status an install never records,
        // must not be believed.
        for altered in ["Verified publisher", "Not yet checked", "trusted"] {
            let mut value = original.clone();
            value["trust"] = altered.into();
            fs::write(&manifest_path, serde_json::to_vec(&value).unwrap()).unwrap();
            let listed = &list_installed_snapshots(&root).unwrap()[0];
            assert_eq!(listed.trust, TrustStatus::VerificationFailed, "{altered}");
            assert!(!listed.damaged);
        }
    }

    #[test]
    fn interrupted_removal_leaves_only_reclaimable_staging_residue() {
        let dir = test_dir("remove-crash");
        let root = dir.0.join("root");
        fs::create_dir_all(&root).unwrap();
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3023, true);
        install(&root, &pack_bytes(&tree, "doomed")).unwrap();
        // Fail the first sync after the rename: the snapshot is already out
        // of snapshots/ and only staging residue remains.
        let result = remove_installed_snapshot_with_sync(&root, "doomed", |_| {
            Err(io::Error::from(io::ErrorKind::StorageFull))
        });
        assert!(matches!(result, Err(PackInstallError::Io(_))));
        assert!(!root.join("snapshots/doomed").exists());
        assert!(list_installed_snapshots(&root).unwrap().is_empty());
        assert_eq!(staging_entries(&root), 1);
        assert_eq!(reclaim_abandoned_staging(&root).unwrap(), (1, 0));
        assert_eq!(staging_entries(&root), 0);
    }

    #[test]
    fn sync_failures_never_publish_a_partial_snapshot() {
        let dir = test_dir("sync");
        let tree = dir.0.join("tree");
        snapshot_tree(&tree, 3014, true);
        let bytes = pack_bytes(&tree, "mars-sync");
        // Sync order on success: staged tree, snapshots/, staging/.
        for fail_at in 0..3 {
            let root = dir.0.join(format!("root-{fail_at}"));
            fs::create_dir_all(&root).unwrap();
            let mut calls = 0;
            let result = install_with_sync(
                &root,
                None,
                |_| true,
                |parent, name| {
                    stage::stage_container(&mut Cursor::new(bytes.as_slice()), parent, name)
                },
                |path| {
                    let call = calls;
                    calls += 1;
                    if call == fail_at {
                        Err(io::Error::from(io::ErrorKind::StorageFull))
                    } else {
                        sync_dir(path)
                    }
                },
            );
            assert!(result.is_err(), "fail_at {fail_at}");
            let published = root.join("snapshots/mars-sync");
            assert_eq!(staging_entries(&root), 0, "fail_at {fail_at}");
            if fail_at == 0 {
                // Before the rename the staged tree is removed durably.
                assert!(!published.exists());
                install(&root, &bytes).unwrap();
            } else {
                // After the rename the complete, verified tree is in place but
                // inactive; a retry must not replace it.
                assert!(published.join(MANIFEST_FILE).exists());
                assert!(matches!(
                    install(&root, &bytes),
                    Err(PackInstallError::AlreadyInstalled)
                ));
            }
            let store = ActiveSnapshotStore::new(&root);
            assert_eq!(store.current().unwrap(), None);
            store.switch_to_prevalidated("mars-sync").unwrap();
        }
    }
}
