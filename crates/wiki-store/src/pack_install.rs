//! STORE-003 custom `.wpack` installer: stage, verify, confirm, publish.
//!
//! A pack's objects mirror the snapshot layout directly:
//! `catalog.sqlite`, the article shards the catalog names, and optionally a
//! derived `search.sqlite`. The installer
//!
//! 1. stages the envelope through `wiki_pack_format::stage` into the private
//!    `<root>/staging/` directory (two-pass digest verification, 0700/0600);
//! 2. refuses an `Origin::Official` claim outright: until TRUST-001 verifies
//!    publisher signatures, an official label cannot be earned by hashes;
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
use wiki_pack_format::manifest::{Manifest, Origin};
use wiki_pack_format::stage;
use wiki_pack_format::trust::{classify, IntegrityState, TrustStatus};

use crate::activation::safe_id;
use crate::catalog::SnapshotCatalog;
use crate::offline_reader::{ReaderError, VerifiedOfflineCatalogReader};
use crate::search_index::{OfflineSearchIndex, SearchError};

pub const CATALOG_OBJECT: &str = "catalog.sqlite";
pub const SEARCH_OBJECT: &str = "search.sqlite";
pub const MANIFEST_FILE: &str = "manifest.json";

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
    /// An unsigned official-origin claim; refused until TRUST-001.
    UnsignedOfficialPack,
    /// `pack_id` is not a valid snapshot directory name.
    InvalidSnapshotId,
    /// An object the snapshot layout reserves or cannot use.
    UnsupportedObject(String),
    MissingCatalog,
    Search(SearchError),
    Reader(ReaderError),
    AlreadyInstalled,
    Declined,
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
    install_with(root, confirm, |parent, name| {
        stage::stage_container_with_budget(input, parent, name, max_input_bytes)
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
    install_with(root, confirm, |parent, name| {
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

/// Remove staging residue an interrupted install left behind: `install-*`
/// directories and `.wpack-spool-*` files in the private `staging/`
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
        if name.starts_with("install-") && kind.is_dir() {
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
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
    stage_into: impl FnOnce(&Path, &str) -> Result<(Manifest, PathBuf), ContainerError>,
) -> Result<InstalledPack, PackInstallError> {
    install_with_sync(root, confirm, stage_into, sync_dir)
}

// Injectable directory-sync seam for fault tests of publication.
fn install_with_sync(
    root: &Path,
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

    let prepared = prepare_staged(&manifest, &staged, &snapshots, confirm, &mut sync);
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
    staged: &Path,
    snapshots: &Path,
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
    sync: &mut impl FnMut(&Path) -> io::Result<()>,
) -> Result<(String, TrustStatus, ShardInventory), PackInstallError> {
    if !matches!(manifest.origin, Origin::Custom { .. }) {
        return Err(PackInstallError::UnsignedOfficialPack);
    }
    manifest
        .validate()
        .map_err(|error| PackInstallError::Container(ContainerError::Manifest(error)))?;
    // Staging rehashed every object byte, so integrity has passed.
    let trust = classify(manifest, IntegrityState::Passed);
    let id = manifest.pack_id.clone();
    if !safe_id(&id) {
        return Err(PackInstallError::InvalidSnapshotId);
    }

    let mut has_catalog = false;
    let mut has_search = false;
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
        match path {
            CATALOG_OBJECT => has_catalog = true,
            SEARCH_OBJECT => has_search = true,
            _ => {
                shards.insert(
                    object.path.clone(),
                    (object.bytes, object.sha256.to_ascii_lowercase()),
                );
            }
        }
    }
    if !has_catalog {
        return Err(PackInstallError::MissingCatalog);
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

    write_snapshot_manifest(staged, manifest)?;
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

/// The installer, not the pack, owns `manifest.json`: it records the verified
/// envelope identity and object inventory the snapshot was installed from.
fn write_snapshot_manifest(staged: &Path, manifest: &Manifest) -> io::Result<()> {
    let definition_id = match &manifest.origin {
        Origin::Custom { definition_id } => definition_id.as_str(),
        Origin::Official { .. } => unreachable!("official packs are refused earlier"),
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
        "origin": { "custom": { "definition_id": definition_id } },
        "trust": TrustStatus::CustomIntegrityVerified.label(),
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

    fn pack_bytes(tree: &Path, pack_id: &str) -> Vec<u8> {
        let manifest =
            custom_manifest_from_tree(tree, pack_id, "enwiki", "20261101", "mars-def").unwrap();
        let mut bytes = Vec::new();
        write_custom_tree(tree, &manifest, &mut bytes).unwrap();
        bytes
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
        let mut manifest =
            custom_manifest_from_tree(&tree, "official-pack", "enwiki", "20261101", "x").unwrap();
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
