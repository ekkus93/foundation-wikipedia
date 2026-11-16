//! MOBPACK-001: discover official packs in the signed catalog, download one
//! resumably and install it only after its publisher signature and every
//! byte verify (ADR 0008, ADR 0009).
//!
//! Trust never comes from the network: the catalog must verify against the
//! pinned keyring, its sequence may not go backwards, the pack's signature
//! file must name the transcript the catalog lists, and the installer checks
//! that signature and every object again before publishing the snapshot.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wiki_pack_format::publisher_catalog::{
    self, Catalog, CatalogError, CatalogPack, CatalogSignature, MAX_CATALOG_BYTES,
};
use wiki_pack_format::signature::{hex, PackSignature, TrustedKey, MAX_SIGNATURE_FILE_BYTES};
use wiki_store::pack_install::{
    install_official_pack, InstalledPack, PackInstallError, PackInstallReview, PublisherProof,
};

use crate::pack_download::{download_resumable, fetch_small, Connector, DownloadError};

const CATALOG_DIR: &str = "official-catalog";
/// The last verified catalog: its signature file followed by its bytes.
const CATALOG_FILE: &str = "verified-catalog";
const DOWNLOADS_DIR: &str = "downloads";
const CATALOG_SIGNATURE_LINES: usize = 4;

/// Where official packs come from and which keys are trusted.
pub struct OfficialSource {
    pub connector: Arc<dyn Connector>,
    pub keyring: Vec<TrustedKey>,
    pub catalog_url: String,
    pub catalog_signature_url: String,
}

#[derive(Debug)]
pub enum OfficialError {
    Download(DownloadError),
    Catalog(CatalogError),
    /// No verified catalog is stored, or it does not list this pack.
    NotInCatalog,
    /// The pack's signature file is not for the pack the catalog lists, or
    /// the verified pack is not the one the catalog lists.
    Mismatch,
    Install(PackInstallError),
    Storage,
}

impl From<DownloadError> for OfficialError {
    fn from(error: DownloadError) -> Self {
        Self::Download(error)
    }
}

impl From<CatalogError> for OfficialError {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl From<PackInstallError> for OfficialError {
    fn from(error: PackInstallError) -> Self {
        Self::Install(error)
    }
}

fn catalog_path(root: &Path) -> PathBuf {
    root.join(CATALOG_DIR).join(CATALOG_FILE)
}

/// Split a stored record into (signature, catalog bytes).
fn split_record(record: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut newlines = 0;
    let end = record.iter().position(|b| {
        newlines += usize::from(*b == b'\n');
        newlines == CATALOG_SIGNATURE_LINES
    })? + 1;
    Some(record.split_at(end))
}

/// The last verified catalog, verified again now (the keyring may have
/// changed since it was stored). `None` if absent or no longer valid.
pub fn stored_catalog(root: &Path, keyring: &[TrustedKey]) -> Option<Catalog> {
    let path = catalog_path(root);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > (MAX_CATALOG_BYTES + MAX_SIGNATURE_FILE_BYTES) as u64 {
        return None;
    }
    let record = fs::read(&path).ok()?;
    let (signature, bytes) = split_record(&record)?;
    let signature = CatalogSignature::parse(signature).ok()?;
    publisher_catalog::verify(bytes, &signature, keyring, None).ok()
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), OfficialError> {
    let parent = path.parent().ok_or(OfficialError::Storage)?;
    fs::create_dir_all(parent).map_err(|_| OfficialError::Storage)?;
    let temporary = parent.join(".incoming");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|_| OfficialError::Storage)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| OfficialError::Storage)?;
    fs::rename(&temporary, path).map_err(|_| OfficialError::Storage)?;
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| OfficialError::Storage)
}

/// Fetch and verify the current catalog. An older catalog than the one
/// already stored is refused (rollback); the verified one replaces it.
pub fn refresh_catalog(
    root: &Path,
    source: &OfficialSource,
    cancelled: &dyn Fn() -> bool,
) -> Result<Catalog, OfficialError> {
    let accepted = stored_catalog(root, &source.keyring).map(|catalog| catalog.sequence);
    let signature_bytes = fetch_small(
        source.connector.as_ref(),
        &source.catalog_signature_url,
        MAX_SIGNATURE_FILE_BYTES,
        cancelled,
    )?;
    let signature = CatalogSignature::parse(&signature_bytes)
        .map_err(|error| OfficialError::Catalog(CatalogError::Signature(error)))?;
    let bytes = fetch_small(
        source.connector.as_ref(),
        &source.catalog_url,
        MAX_CATALOG_BYTES,
        cancelled,
    )?;
    let catalog = publisher_catalog::verify(&bytes, &signature, &source.keyring, accepted)?;
    let mut record = signature_bytes;
    record.extend_from_slice(&bytes);
    write_atomically(&catalog_path(root), &record)?;
    Ok(catalog)
}

/// Only plain names become download file names.
fn download_name(pack_id: &str) -> Option<String> {
    let ok = !pack_id.is_empty()
        && pack_id.len() <= 128
        && !pack_id.starts_with('.')
        && pack_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    ok.then(|| format!("{pack_id}.wpack.part"))
}

/// The partial download of `pack_id`, if any, for progress display.
pub fn partial_bytes(root: &Path, pack_id: &str) -> u64 {
    download_name(pack_id)
        .and_then(|name| fs::metadata(root.join(DOWNLOADS_DIR).join(name)).ok())
        .map_or(0, |meta| meta.len())
}

/// Download `pack_id` from the stored verified catalog, then install it as
/// an official pack. `confirm` sees the fully verified pack (labelled
/// Verified publisher) before it is published. The partial download is kept
/// after a network failure so a retry resumes; it is deleted once the pack
/// installs, fails verification or is declined.
pub fn download_and_install(
    root: &Path,
    source: &OfficialSource,
    pack_id: &str,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(u64, u64),
    confirm: impl FnOnce(&PackInstallReview<'_>) -> bool,
) -> Result<InstalledPack, OfficialError> {
    let catalog = stored_catalog(root, &source.keyring).ok_or(OfficialError::NotInCatalog)?;
    let entry: CatalogPack = catalog
        .packs
        .into_iter()
        .find(|pack| pack.pack_id == pack_id)
        .ok_or(OfficialError::NotInCatalog)?;
    let name = download_name(&entry.pack_id).ok_or(OfficialError::Mismatch)?;
    if root.join("snapshots").join(&entry.pack_id).exists() {
        return Err(PackInstallError::AlreadyInstalled.into());
    }

    let signature = PackSignature::parse(&fetch_small(
        source.connector.as_ref(),
        &entry.signature_url,
        MAX_SIGNATURE_FILE_BYTES,
        cancelled,
    )?)
    .map_err(|_| OfficialError::Mismatch)?;
    if hex(&signature.transcript_sha256) != entry.transcript_sha256 {
        return Err(OfficialError::Mismatch);
    }

    let downloads = root.join(DOWNLOADS_DIR);
    fs::create_dir_all(&downloads).map_err(|_| OfficialError::Storage)?;
    let partial = downloads.join(name);
    download_resumable(
        source.connector.as_ref(),
        &entry.url,
        entry.bytes,
        &partial,
        cancelled,
        progress,
    )?;

    let mut file = File::open(&partial).map_err(|_| OfficialError::Storage)?;
    let mut mismatch = false;
    let result = install_official_pack(
        root,
        &mut file,
        entry.bytes,
        PublisherProof {
            signature: &signature,
            keyring: &source.keyring,
        },
        |review| {
            let manifest = review.manifest;
            if manifest.pack_id != entry.pack_id
                || manifest.project != entry.project
                || manifest.snapshot != entry.snapshot
            {
                mismatch = true;
                return false;
            }
            confirm(review)
        },
    );
    drop(file);
    // Every outcome past the byte download is final for these bytes.
    let _ = fs::remove_file(&partial);
    match result {
        Ok(installed) => Ok(installed),
        Err(_) if mismatch => Err(OfficialError::Mismatch),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack_download::tests::{fake_github, ok, ranged, redirect, ASSET_URL};
    use crate::pack_download::{RELEASE_HOST, RELEASE_PATH_PREFIX};
    use std::sync::Mutex;
    use wiki_pack_format::publisher_catalog::CATALOG_FORMAT;
    use wiki_pack_format::signature::{self, public_key};
    use wiki_pack_format::trust::TrustStatus;

    /// Throwaway test key; never pinned in the app.
    const SECRET: [u8; 32] = [7; 32];

    fn keyring() -> Vec<TrustedKey> {
        vec![TrustedKey {
            publisher: "foundation",
            public_key: public_key(&SECRET),
            valid_from: "20260101",
            valid_until: None,
            revoked: false,
        }]
    }

    fn url(name: &str) -> String {
        format!("https://{RELEASE_HOST}{RELEASE_PATH_PREFIX}packs-20261101/{name}")
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "official-packs-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }

    /// A one-article official pack tree exported as `.wpack` bytes.
    fn official_pack_bytes(dir: &Path, pack_id: &str) -> Vec<u8> {
        use wiki_model::{
            Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
        };
        let tree = dir.join(format!("{pack_id}-tree"));
        let _ = fs::remove_dir_all(&tree);
        fs::create_dir_all(&tree).unwrap();
        let article = Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 1,
            },
            revision: Revision {
                revision_id: 9001,
                timestamp: "2026-11-01T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Atom".into(),
            display_title: "Atom".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Atoms make up matter.".into()),
            }],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article><p>Atoms make up matter.</p></article>".into(),
            is_disambiguation: false,
        };
        let mut shard = Vec::new();
        wiki_store::record_codec::append_record_frame(
            &mut shard,
            &PageRecord::Article(Box::new(article)),
        )
        .unwrap();
        fs::write(tree.join("articles.shard"), &shard).unwrap();
        let mut catalog =
            wiki_store::catalog::SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
        catalog
            .insert_verified(&tree, "articles.shard", 0, shard.len() as u64)
            .unwrap();
        drop(catalog);
        let metadata = wiki_store::pack_install::snapshot_pack_metadata(
            &tree,
            wiki_pack_format::manifest::PackMetadata::synthetic_fixture().source,
            &"0".repeat(64),
            "CC-BY-SA-4.0",
        )
        .unwrap();
        let manifest = wiki_pack_format::export::official_manifest_from_tree(
            &tree,
            pack_id,
            "enwiki",
            "20261101",
            "foundation",
            metadata,
        )
        .unwrap();
        let mut bytes = Vec::new();
        wiki_pack_format::export::write_official_tree(&tree, &manifest, &mut bytes).unwrap();
        bytes
    }

    /// A signed official pack: (pack bytes, pack signature file).
    fn official_pack(dir: &Path, pack_id: &str) -> (Vec<u8>, Vec<u8>) {
        let bytes = official_pack_bytes(dir, pack_id);
        let manifest =
            wiki_pack_format::container::verify_container(&mut bytes.as_slice()).unwrap();
        let signed = signature::sign(&manifest, &SECRET).unwrap();
        (bytes, signed.encode().into_bytes())
    }

    struct Published {
        catalog: Vec<u8>,
        catalog_sig: Vec<u8>,
        pack: Vec<u8>,
        pack_sig: Vec<u8>,
    }

    fn publish(dir: &Path, sequence: u64) -> Published {
        let (pack, pack_sig) = official_pack(dir, "science");
        let parsed = PackSignature::parse(&pack_sig).unwrap();
        let catalog = Catalog {
            format: CATALOG_FORMAT.into(),
            publisher: "foundation".into(),
            sequence,
            generated: "20261116".into(),
            packs: vec![CatalogPack {
                pack_id: "science".into(),
                project: "enwiki".into(),
                snapshot: "20261101".into(),
                title: "Science".into(),
                url: url("science.wpack"),
                bytes: pack.len() as u64,
                signature_url: url("science.wpack.sig"),
                transcript_sha256: hex(&parsed.transcript_sha256),
            }],
        }
        .to_bytes()
        .unwrap();
        let catalog_sig = publisher_catalog::sign(&catalog, &SECRET)
            .unwrap()
            .encode()
            .into_bytes();
        Published {
            catalog,
            catalog_sig,
            pack,
            pack_sig,
        }
    }

    fn serve(
        published: Arc<Mutex<Published>>,
    ) -> (OfficialSource, crate::pack_download::tests::Seen) {
        let server = fake_github(move |sni, head| {
            let published = published.lock().unwrap();
            if sni != RELEASE_HOST {
                return ranged(&published.pack, head);
            }
            let path = head.split(' ').nth(1).unwrap_or_default();
            if path.ends_with("/catalog.json") {
                ok(&published.catalog)
            } else if path.ends_with("/catalog.json.sig") {
                ok(&published.catalog_sig)
            } else if path.ends_with("/science.wpack.sig") {
                ok(&published.pack_sig)
            } else if path.ends_with("/science.wpack") {
                redirect(ASSET_URL)
            } else {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
            }
        });
        (
            OfficialSource {
                connector: Arc::new(server.connector),
                keyring: keyring(),
                catalog_url: url("catalog.json").replace("packs-20261101", "catalog"),
                catalog_signature_url: url("catalog.json.sig").replace("packs-20261101", "catalog"),
            },
            server.seen,
        )
    }

    const NEVER: &dyn Fn() -> bool = &|| false;

    #[test]
    fn a_catalog_pack_downloads_verifies_and_installs_as_verified_publisher() {
        let dir = temp("install");
        let root = dir.join("library");
        fs::create_dir_all(&root).unwrap();
        let published = Arc::new(Mutex::new(publish(&dir, 2)));
        let (source, _) = serve(published.clone());

        assert!(stored_catalog(&root, &source.keyring).is_none());
        let catalog = refresh_catalog(&root, &source, NEVER).unwrap();
        assert_eq!(catalog.packs[0].pack_id, "science");
        assert_eq!(stored_catalog(&root, &source.keyring), Some(catalog));

        let mut reviewed = None;
        let installed =
            download_and_install(&root, &source, "science", NEVER, &|_, _| {}, |review| {
                reviewed = Some(review.trust);
                true
            })
            .unwrap();
        assert_eq!(reviewed, Some(TrustStatus::VerifiedPublisher));
        assert_eq!(installed.trust, TrustStatus::VerifiedPublisher);
        assert_eq!(
            wiki_store::pack_install::list_installed_snapshots_with_keyring(&root, &keyring())
                .unwrap()[0]
                .trust,
            TrustStatus::VerifiedPublisher
        );
        assert_eq!(partial_bytes(&root, "science"), 0);
        assert!(matches!(
            download_and_install(&root, &source, "science", NEVER, &|_, _| {}, |_| true),
            Err(OfficialError::Install(PackInstallError::AlreadyInstalled))
        ));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_older_or_tampered_catalog_is_refused_and_the_stored_one_kept() {
        let dir = temp("rollback");
        let root = dir.join("library");
        fs::create_dir_all(&root).unwrap();
        let published = Arc::new(Mutex::new(publish(&dir, 5)));
        let (source, _) = serve(published.clone());
        refresh_catalog(&root, &source, NEVER).unwrap();

        *published.lock().unwrap() = publish(&dir, 4);
        assert!(matches!(
            refresh_catalog(&root, &source, NEVER),
            Err(OfficialError::Catalog(CatalogError::Rollback {
                accepted: 5,
                offered: 4
            }))
        ));
        published.lock().unwrap().catalog.push(b' ');
        assert!(matches!(
            refresh_catalog(&root, &source, NEVER),
            Err(OfficialError::Catalog(CatalogError::DigestMismatch))
        ));
        assert_eq!(stored_catalog(&root, &source.keyring).unwrap().sequence, 5);

        // Under a keyring without the signing key nothing stored is trusted.
        assert!(stored_catalog(&root, &[]).is_none());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_substituted_or_tampered_pack_never_installs() {
        let dir = temp("tamper");
        let root = dir.join("library");
        fs::create_dir_all(&root).unwrap();
        let published = Arc::new(Mutex::new(publish(&dir, 1)));
        let (source, _) = serve(published.clone());
        refresh_catalog(&root, &source, NEVER).unwrap();

        // A signature for another pack (here: re-signed tampered transcript).
        let other_dir = dir.join("other");
        fs::create_dir_all(&other_dir).unwrap();
        let (_, other_sig) = official_pack(&other_dir, "other");
        let good_sig = std::mem::replace(&mut published.lock().unwrap().pack_sig, other_sig);
        assert!(matches!(
            download_and_install(&root, &source, "science", NEVER, &|_, _| {}, |_| true),
            Err(OfficialError::Mismatch)
        ));

        // The right signature, but one flipped pack byte.
        {
            let mut published = published.lock().unwrap();
            published.pack_sig = good_sig;
            *published.pack.last_mut().unwrap() ^= 1;
        }
        let result = download_and_install(&root, &source, "science", NEVER, &|_, _| {}, |_| true);
        assert!(
            matches!(result, Err(OfficialError::Install(_))),
            "{result:?}"
        );
        assert_eq!(
            partial_bytes(&root, "science"),
            0,
            "bad bytes are discarded"
        );
        assert!(wiki_store::pack_install::list_installed_snapshots(&root)
            .unwrap()
            .is_empty());

        // Unknown packs are refused without any request.
        assert!(matches!(
            download_and_install(&root, &source, "missing", NEVER, &|_, _| {}, |_| true),
            Err(OfficialError::NotInCatalog)
        ));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_declined_install_discards_the_download_and_a_failure_keeps_it() {
        let dir = temp("declined");
        let root = dir.join("library");
        fs::create_dir_all(&root).unwrap();
        let published = Arc::new(Mutex::new(publish(&dir, 1)));
        let (source, seen) = serve(published.clone());
        refresh_catalog(&root, &source, NEVER).unwrap();
        assert!(matches!(
            download_and_install(&root, &source, "science", NEVER, &|_, _| {}, |_| false),
            Err(OfficialError::Install(PackInstallError::Declined))
        ));
        assert_eq!(partial_bytes(&root, "science"), 0);

        // An interrupted download: what arrived is kept and resumed.
        let total = published.lock().unwrap().pack.len() as u64;
        let partial = root.join(DOWNLOADS_DIR).join("science.wpack.part");
        fs::write(&partial, &published.lock().unwrap().pack[..100]).unwrap();
        seen.lock().unwrap().clear();
        let installed =
            download_and_install(&root, &source, "science", NEVER, &|_, _| {}, |_| true).unwrap();
        assert_eq!(installed.snapshot_id, "science");
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|(_, head)| head.contains("Range: bytes=100-")));
        assert!(total > 100);
        fs::remove_dir_all(dir).ok();
    }
}
