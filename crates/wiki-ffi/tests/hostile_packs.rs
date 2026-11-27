//! TRUST-003: the hostile-pack suite. Every pack here is built in code and
//! pushed through the real install paths (`OfflineLibrary::install_pack`,
//! `install_pack_stream`, and the store installers for the exact error).
//! Each attack must end in a typed error, never a panic; leave the library
//! tree byte-for-byte as it was (nothing written outside it, no staging
//! residue); and leave an already-installed good pack usable.
//!
//! The format is the binary `FWPACK01` envelope (ADR 0007): a canonical
//! manifest transcript, then raw objects. There is no archive compression,
//! so "bomb" attacks are declared-size, count and length-field lies, plus
//! oversized and pathological image/SVG objects.
//!
//! Contract for article HTML: a pack stores `rendered_html` verbatim and the
//! reader document is sanitised again at display time, whatever the pack
//! claims. The HTML tests therefore install the pack and assert the reader
//! output. Images and SVGs are checked at install time and refused.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use support::{blue_svg, build_image_pack_with_svg, build_pack, test_dir, RED_PNG};
use wiki_ffi::android::{
    InstallReviewer, LibraryError, OfflineLibrary, PackReview, PackSource, PackTrust,
};
use wiki_model::{MediaAsset, MediaDerivation, PageRecord};
use wiki_pack_format::container::{verify_container, ContainerError};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::{
    IndexRole, Manifest, ManifestError, Object, Origin, PackMetadata, TRANSCRIPT_PREFIX,
};
use wiki_pack_format::signature::{self, SignatureError, TrustedKey};
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::pack_install::{
    install_custom_pack, install_custom_pack_stream, install_official_pack,
    install_official_pack_stream, remove_installed_snapshot, snapshot_pack_metadata,
    PackInstallError, PublisherProof,
};
use wiki_store::pack_media::{
    encode_media_index, media_object_path, PackImage, PackMediaError, MAX_PACK_IMAGE_BYTES,
    MEDIA_DIR, MEDIA_INDEX_OBJECT, PACK_IMAGE_CSP,
};
use wiki_store::record_codec::append_record_frame;

const MAGIC: &[u8; 8] = b"FWPACK01";

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// ---------------------------------------------------------------------------
// Raw envelope construction. The real writer refuses to emit hostile
// manifests, so these helpers encode the transcript by hand, without
// validating it, exactly as an attacker's tool would.

struct Offsets {
    pack_id_len: usize,
    object_count: usize,
}

fn put_field(out: &mut Vec<u8>, field: &str) {
    out.extend_from_slice(&(field.len() as u64).to_be_bytes());
    out.extend_from_slice(field.as_bytes());
}

/// The transcript of `manifest` exactly as given: no validation, objects in
/// the given order, digests as written.
fn encode_transcript(manifest: &Manifest) -> (Vec<u8>, Offsets) {
    let mut out = TRANSCRIPT_PREFIX.to_vec();
    out.extend_from_slice(&manifest.version.to_be_bytes());
    let pack_id_len = out.len();
    put_field(&mut out, &manifest.pack_id);
    put_field(&mut out, &manifest.project);
    put_field(&mut out, &manifest.snapshot);
    match &manifest.origin {
        Origin::Official { publisher } => {
            out.push(1);
            put_field(&mut out, publisher);
        }
        Origin::Custom { definition_id } => {
            out.push(2);
            put_field(&mut out, definition_id);
        }
    }
    let meta = &manifest.metadata;
    out.push(meta.source.kind.tag());
    put_field(&mut out, &meta.source.snapshot_id);
    put_field(&mut out, &meta.source.manifest_sha256);
    put_field(&mut out, &meta.definition_sha256);
    put_field(&mut out, &meta.license.text_spdx);
    match &meta.license.attribution_object {
        None => out.push(0),
        Some(path) => {
            out.push(1);
            put_field(&mut out, path);
        }
    }
    let mut indexes = meta.indexes.clone();
    indexes.sort_by(|a, b| a.path.cmp(&b.path));
    out.extend_from_slice(&(indexes.len() as u64).to_be_bytes());
    for index in &indexes {
        put_field(&mut out, &index.path);
        out.push(index.role.tag());
        out.extend_from_slice(&index.schema.to_be_bytes());
    }
    out.extend_from_slice(&meta.counts.articles.to_be_bytes());
    out.extend_from_slice(&meta.counts.redirects.to_be_bytes());
    out.extend_from_slice(&meta.counts.media.to_be_bytes());
    let object_count = out.len();
    out.extend_from_slice(&(manifest.objects.len() as u64).to_be_bytes());
    for object in &manifest.objects {
        put_field(&mut out, &object.path);
        put_field(&mut out, &object.sha256);
        out.extend_from_slice(&object.bytes.to_be_bytes());
    }
    (
        out,
        Offsets {
            pack_id_len,
            object_count,
        },
    )
}

fn frame(transcript: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(transcript.len() as u32).to_be_bytes());
    out.extend_from_slice(transcript);
    out.extend_from_slice(payload);
    out
}

/// A good pack split into its manifest and its objects (path order).
#[derive(Clone)]
struct Parts {
    manifest: Manifest,
    objects: Vec<(String, Vec<u8>)>,
}

impl Parts {
    fn of(pack: &[u8]) -> Self {
        let manifest = verify_container(&mut Cursor::new(pack)).expect("good pack verifies");
        let length = u32::from_be_bytes(pack[8..12].try_into().unwrap()) as usize;
        let mut at = 12 + length;
        let mut sorted: Vec<&Object> = manifest.objects.iter().collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut objects = Vec::new();
        for object in sorted {
            let end = at + object.bytes as usize;
            objects.push((object.path.clone(), pack[at..end].to_vec()));
            at = end;
        }
        assert_eq!(at, pack.len());
        Self { manifest, objects }
    }

    fn object(&self, path: &str) -> &[u8] {
        &self
            .objects
            .iter()
            .find(|(name, _)| name == path)
            .unwrap_or_else(|| panic!("no object {path}"))
            .1
    }

    /// Reassemble after `objects` and `manifest` edits. The manifest's
    /// object list is rebuilt (sorted, honest) from the edited objects first,
    /// so only what `edit_manifest` then changes is a lie.
    fn pack(
        &self,
        edit_objects: impl FnOnce(&mut Vec<(String, Vec<u8>)>),
        edit_manifest: impl FnOnce(&mut Manifest),
    ) -> Vec<u8> {
        self.pack_with(edit_objects, edit_manifest, |_, _| {})
    }

    fn pack_with(
        &self,
        edit_objects: impl FnOnce(&mut Vec<(String, Vec<u8>)>),
        edit_manifest: impl FnOnce(&mut Manifest),
        edit_wire: impl FnOnce(&mut Vec<u8>, &Offsets),
    ) -> Vec<u8> {
        let mut objects = self.objects.clone();
        edit_objects(&mut objects);
        objects.sort_by(|a, b| a.0.cmp(&b.0));
        let mut manifest = self.manifest.clone();
        manifest.objects = objects
            .iter()
            .map(|(path, bytes)| Object {
                path: path.clone(),
                sha256: hex(bytes),
                bytes: bytes.len() as u64,
            })
            .collect();
        edit_manifest(&mut manifest);
        let (mut transcript, offsets) = encode_transcript(&manifest);
        edit_wire(&mut transcript, &offsets);
        let payload: Vec<u8> = objects.iter().flat_map(|(_, b)| b.clone()).collect();
        frame(&transcript, &payload)
    }

    fn honest(&self) -> Vec<u8> {
        self.pack(|_| {}, |_| {})
    }
}

// ---------------------------------------------------------------------------
// A custom pack builder with arbitrary article HTML and arbitrary image
// bytes under any declared type, all hashes valid.

fn build_custom(
    dir: &Path,
    pack_id: &str,
    html: &str,
    images: &[(&str, Vec<u8>)],
    index: Option<Vec<u8>>,
) -> Vec<u8> {
    build_custom_licensed(dir, pack_id, html, images, index, "CC-BY-SA-4.0")
}

fn build_custom_licensed(
    dir: &Path,
    pack_id: &str,
    html: &str,
    images: &[(&str, Vec<u8>)],
    index: Option<Vec<u8>>,
    text_spdx: &str,
) -> Vec<u8> {
    let tree = dir.join(format!("{pack_id}-tree"));
    // A fresh tree every time: callers reuse ids across loop iterations.
    let _ = fs::remove_dir_all(&tree);
    fs::create_dir_all(&tree).unwrap();
    if !images.is_empty() {
        fs::create_dir_all(tree.join(MEDIA_DIR)).unwrap();
    }
    let mut page = support::article(5, "Hostile", "A page with a hostile body.");
    page.revision.revision_id = 7005;
    page.rendered_html = html.to_owned();
    for (n, (mime, bytes)) in images.iter().enumerate() {
        page.media.push(MediaAsset {
            source_url: format!("https://upload.wikimedia.org/wikipedia/commons/a/ab/Img{n}.png"),
            mime_type: (*mime).into(),
            sha256: Some(hex(bytes)),
            license: "CC0".into(),
            creator: "Synthetic".into(),
            attribution: "Synthetic, CC0".into(),
            is_av_preview: false,
            derivation: (*mime == "image/svg+xml").then_some(MediaDerivation::SanitizedSvg),
        });
        fs::write(tree.join(media_object_path(&hex(bytes))), bytes).unwrap();
    }
    let mut shard = Vec::new();
    append_record_frame(&mut shard, &PageRecord::Article(Box::new(page))).unwrap();
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    catalog
        .insert_verified(&tree, "articles.shard", 0, shard.len() as u64)
        .unwrap();
    drop(catalog);
    // Metadata first, without the index: a hostile index would be refused by
    // the builder's own parser, which is not the path under test.
    let mut metadata: PackMetadata = snapshot_pack_metadata(
        &tree,
        PackMetadata::synthetic_fixture().source,
        &"0".repeat(64),
        text_spdx,
    )
    .unwrap();
    if !images.is_empty() || index.is_some() {
        let index = index.unwrap_or_else(|| {
            encode_media_index(
                &images
                    .iter()
                    .map(|(mime, bytes)| PackImage {
                        sha256: hex(bytes),
                        mime: (*mime).into(),
                        bytes: bytes.len() as u64,
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        });
        fs::write(tree.join(MEDIA_INDEX_OBJECT), index).unwrap();
        metadata.license.attribution_object = Some(MEDIA_INDEX_OBJECT.into());
        metadata.counts.media = images.len() as u64;
    }
    let manifest =
        custom_manifest_from_tree(&tree, pack_id, "enwiki", "20261101", "hostile", metadata)
            .unwrap();
    let mut out = Vec::new();
    write_custom_tree(&tree, &manifest, &mut out).unwrap();
    out
}

// ---------------------------------------------------------------------------
// The lab: a library with one good pack installed and active.

struct VecSource(Mutex<Cursor<Vec<u8>>>);

impl PackSource for VecSource {
    fn read(&self, max_bytes: u32) -> Result<Vec<u8>, LibraryError> {
        let mut buffer = vec![0; max_bytes.min(64 * 1024) as usize];
        let count = self.0.lock().unwrap().read(&mut buffer).unwrap();
        buffer.truncate(count);
        Ok(buffer)
    }
}

struct Approve;

impl InstallReviewer for Approve {
    fn approve(&self, _review: PackReview) -> bool {
        true
    }
}

/// Every entry under `dir` (relative path, kind, length), without following
/// symlinks, so any file written anywhere inside the lab shows up.
fn tree(dir: &Path) -> BTreeMap<String, (char, u64)> {
    fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, (char, u64)>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            let kind = if meta.file_type().is_symlink() {
                'l'
            } else if meta.is_dir() {
                'd'
            } else {
                'f'
            };
            let relative = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            out.insert(relative, (kind, if kind == 'f' { meta.len() } else { 0 }));
            if kind == 'd' {
                walk(base, &path, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Lab {
    dir: support::TestDir,
    library: Arc<OfflineLibrary>,
    /// The installed good pack's bytes: the raw material for most attacks.
    good: Vec<u8>,
}

impl Lab {
    fn new(label: &str) -> Self {
        let dir = test_dir(label);
        fs::create_dir_all(dir.0.join("src")).unwrap();
        fs::create_dir_all(dir.0.join("attacks")).unwrap();
        fs::create_dir_all(dir.0.join("outside")).unwrap();
        let good_path = build_pack(&dir.0.join("src"), "good-pack");
        let good = fs::read(&good_path).unwrap();
        // Nested so a `../../..` escape from staging lands inside the lab.
        let library = OfflineLibrary::new(
            dir.0
                .join("jail/inner/library")
                .to_string_lossy()
                .into_owned(),
        )
        .unwrap();
        library
            .install_pack(good_path.to_string_lossy().into_owned(), true)
            .unwrap();
        library.activate("good-pack".into()).unwrap();
        let lab = Self { dir, library, good };
        lab.assert_good_pack_usable();
        lab
    }

    fn parts(&self) -> Parts {
        Parts::of(&self.good)
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.dir.0.join("jail/inner/library")).unwrap()
    }

    fn assert_good_pack_usable(&self) {
        let packs = self.library.list_packs().unwrap();
        assert_eq!(packs.len(), 1, "{packs:?}");
        assert_eq!(packs[0].snapshot_id, "good-pack");
        assert!(!packs[0].damaged);
        assert!(packs[0].active);
        let page = self.library.open_page("enwiki".into(), 1).unwrap().unwrap();
        assert_eq!(page.title, "Moon");
        assert_eq!(self.library.search("Moon".into(), 5).unwrap().len(), 2);
    }

    /// Run `pack` through the file and stream installers: both must return
    /// `want`, change nothing under the lab, and leave the good pack usable.
    fn refuse(&self, attack: &str, pack: &[u8], want: LibraryError) {
        let file: String = attack
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let path = self.dir.0.join("attacks").join(format!("{file}.wpack"));
        fs::write(&path, pack).unwrap();
        let before = tree(&self.dir.0);
        let by_file = self
            .library
            .install_pack(path.to_string_lossy().into_owned(), true);
        assert_eq!(
            by_file.as_ref().err(),
            Some(&want),
            "{attack}: file install"
        );
        let by_stream = self.library.install_pack_stream(
            Arc::new(VecSource(Mutex::new(Cursor::new(pack.to_vec())))),
            Arc::new(Approve),
            1 << 30,
        );
        assert_eq!(
            by_stream.as_ref().err(),
            Some(&want),
            "{attack}: stream install"
        );
        assert_eq!(
            tree(&self.dir.0),
            before,
            "{attack}: the lab changed (escape or residue)"
        );
        self.assert_good_pack_usable();
    }
}

fn expect_refusal(
    attack: &str,
    result: Result<impl std::fmt::Debug, PackInstallError>,
) -> PackInstallError {
    match result {
        Ok(installed) => panic!("{attack}: hostile pack was installed: {installed:?}"),
        Err(error) => error,
    }
}

/// The precise store-level error, in a fresh private root that must stay
/// empty (no snapshot, no staging residue).
fn store_error(attack: &str, pack: &[u8]) -> PackInstallError {
    let root = test_dir("hostile-store");
    fs::create_dir_all(&root.0).unwrap();
    let root_path = fs::canonicalize(&root.0).unwrap();
    let error = expect_refusal(
        attack,
        install_custom_pack(&root_path, &mut Cursor::new(pack), 1 << 30, |_| true),
    );
    assert_root_clean(attack, &root_path);
    let error_stream = expect_refusal(
        attack,
        install_custom_pack_stream(&root_path, &mut Cursor::new(pack), 1 << 30, |_| true),
    );
    assert_eq!(
        std::mem::discriminant(&error),
        std::mem::discriminant(&error_stream),
        "{attack}: seekable and streamed installs disagree: {error:?} vs {error_stream:?}"
    );
    assert_root_clean(attack, &root_path);
    error
}

fn assert_root_clean(attack: &str, root: &Path) {
    let entries = tree(root);
    let names: Vec<&str> = entries.keys().map(String::as_str).collect();
    assert!(
        names.iter().all(|n| *n == "snapshots" || *n == "staging"),
        "{attack}: unexpected entries {names:?}"
    );
}

// ---------------------------------------------------------------------------
// 1. Path traversal and escape.

fn renamed(parts: &Parts, new_path: &str) -> Vec<u8> {
    parts.pack(
        |objects| {
            objects
                .iter_mut()
                .find(|(path, _)| path == "articles.shard")
                .unwrap()
                .0 = new_path.to_owned()
        },
        |_| {},
    )
}

#[test]
fn path_traversal_and_escape_object_names_are_refused() {
    let lab = Lab::new("hostile-paths");
    let parts = lab.parts();
    let long_component = "a".repeat(256);
    let deep = vec!["d"; 65].join("/");
    let attacks: Vec<(&str, String)> = vec![
        ("dot_dot_slash", "../escape.shard".into()),
        ("deep_dot_dot", "../../../../escape.shard".into()),
        ("embedded_dot_dot", "a/../../escape.shard".into()),
        ("absolute_path", "/tmp/escape.shard".into()),
        ("absolute_system_path", "/etc/cron.d/escape".into()),
        ("backslash_separator", "a\\b.shard".into()),
        ("backslash_traversal", "..\\..\\escape.shard".into()),
        ("windows_drive", "C:evil.shard".into()),
        ("windows_unc", "\\\\server\\share\\x".into()),
        ("nul_byte", "a\0b.shard".into()),
        ("nul_truncation", "articles.shard\0.png".into()),
        ("newline", "a\nb.shard".into()),
        ("dot_component", "a/./b.shard".into()),
        ("empty_component", "a//b.shard".into()),
        ("trailing_slash", "a/".into()),
        ("empty_path", String::new()),
        ("single_dot", ".".into()),
        ("double_dot", "..".into()),
        ("trailing_dot", "name.".into()),
        ("trailing_space", "name ".into()),
        ("windows_device_con", "CON".into()),
        ("windows_device_nul_ext", "nul.txt".into()),
        ("windows_device_padded", "COM1 .txt".into()),
        ("windows_device_superscript", "LPT\u{b2}".into()),
        ("windows_device_nested", "dir/AUX.shard".into()),
        ("windows_device_conout", "CONOUT$".into()),
        ("windows_forbidden_char", "a?b.shard".into()),
        ("ntfs_stream", "a.shard:hidden".into()),
        ("unicode_rtl_override", "a\u{202e}dracs.shard".into()),
        ("unicode_zero_width_space", "art\u{200b}icles.shard".into()),
        ("unicode_bom", "\u{feff}articles.shard".into()),
        ("unicode_decomposed_nfd", "cafe\u{301}.shard".into()),
        ("unicode_noncharacter", "a\u{fdd0}.shard".into()),
        ("overlong_component", format!("{long_component}.shard")),
        ("too_deep", format!("{deep}/x")),
        (
            "overlong_path",
            format!("{}/x", vec!["abcdefgh"; 600].join("/")),
        ),
    ];
    for (name, path) in &attacks {
        let pack = renamed(&parts, path);
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::Manifest(ManifestError::UnsafeObject))
                    | PackInstallError::Container(ContainerError::Manifest(
                        ManifestError::DuplicateObject
                    ))
                    | PackInstallError::Container(ContainerError::ResourceBudget)
            ),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn lookalike_and_case_colliding_object_names_are_refused() {
    let lab = Lab::new("hostile-lookalikes");
    let parts = lab.parts();
    // A second object whose name collides with an existing one on a
    // case-insensitive or normalising filesystem.
    for (name, alias) in [
        ("case_fold_alias", "ARTICLES.SHARD"),
        ("mixed_case_alias", "Articles.shard"),
        ("directory_prefix_alias", "articles.shard/inner"),
        ("sigma_fold_alias", "\u{3c3}\u{3c2}.shard"),
    ] {
        let pack = parts.pack(
            |objects| {
                if name == "sigma_fold_alias" {
                    objects.push(("\u{3c3}\u{3c3}.shard".into(), b"x".to_vec()));
                }
                objects.push((alias.into(), b"payload".to_vec()));
            },
            |_| {},
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::Manifest(
                    ManifestError::DuplicateObject
                ))
            ),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn hostile_pack_identity_fields_cannot_name_a_path_outside_the_library() {
    let lab = Lab::new("hostile-ids");
    let parts = lab.parts();
    // `pack_id` becomes the snapshot directory name; project, snapshot and
    // origin are filesystem-facing identities too.
    let ids = [
        "..",
        ".",
        "../outside",
        "a/b",
        "a\\b",
        "/abs",
        "C:",
        "con",
        "x\0y",
        " padded",
        "ends.",
        "snap\u{202e}shot",
        &"p".repeat(300),
    ];
    for id in ids {
        let pack = parts.pack(|_| {}, |m| m.pack_id = id.to_owned());
        lab.refuse("pack_id", &pack, LibraryError::VerificationFailed);
        let pack = parts.pack(|_| {}, |m| m.snapshot = id.to_owned());
        lab.refuse("snapshot", &pack, LibraryError::VerificationFailed);
        let pack = parts.pack(
            |_| {},
            |m| {
                m.origin = Origin::Custom {
                    definition_id: id.to_owned(),
                }
            },
        );
        lab.refuse("definition_id", &pack, LibraryError::VerificationFailed);
    }
    for project in ["../x", "EnWiki", "en wiki", "", "enwiki/", "1enwiki"] {
        let pack = parts.pack(|_| {}, |m| m.project = project.to_owned());
        lab.refuse("project", &pack, LibraryError::VerificationFailed);
    }
}

#[test]
fn reserved_installer_object_names_are_refused_even_when_declared() {
    let lab = Lab::new("hostile-reserved");
    let parts = lab.parts();
    // Names the installer owns or the snapshot layout reserves: a pack must
    // not ship its own installed-state file or SQLite sidecars.
    for (name, path) in [
        ("installed_manifest_overwrite", "manifest.json"),
        ("sqlite_wal_sidecar", "catalog.sqlite-wal"),
        ("sqlite_shm_sidecar", "catalog.sqlite-shm"),
        ("sqlite_journal_sidecar", "catalog.sqlite-journal"),
        ("fts_scratch_file", ".offline-fts.tmp"),
        ("undeclared_search_index", "search.sqlite"),
    ] {
        let pack = parts.pack(
            |objects| objects.push((path.into(), b"attacker bytes".to_vec())),
            |_| {},
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(error, PackInstallError::UnsupportedObject(_)),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn a_symlinked_staging_or_snapshots_directory_is_refused_and_nothing_follows_it() {
    let lab = Lab::new("hostile-symlinks");
    let root = lab.root();
    let outside = lab.dir.0.join("outside");
    let pack_path = lab.dir.0.join("attacks/again.wpack");
    // A different pack, so AlreadyInstalled is not what stops it.
    let other = build_pack(&lab.dir.0.join("src"), "other-pack");
    fs::rename(other, &pack_path).unwrap();

    for dir in ["staging", "snapshots"] {
        let real = root.join(dir);
        let parked = root.join(format!("{dir}.parked"));
        fs::rename(&real, &parked).unwrap();
        std::os::unix::fs::symlink(&outside, &real).unwrap();
        let result = lab
            .library
            .install_pack(pack_path.to_string_lossy().into_owned(), true);
        assert_eq!(result.err(), Some(LibraryError::Storage), "{dir}");
        assert!(
            fs::read_dir(&outside).unwrap().next().is_none(),
            "{dir}: install wrote through the symlink"
        );
        fs::remove_file(&real).unwrap();
        fs::rename(&parked, &real).unwrap();
    }
    // The store refuses a symlinked root and a symlinked ancestor too.
    let link = lab.dir.0.join("root-link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    let error = expect_refusal(
        "symlinked_root",
        install_custom_pack(&link, &mut Cursor::new(lab.good.clone()), 1 << 30, |_| true),
    );
    assert!(matches!(error, PackInstallError::UnsafeRoot), "{error:?}");
    lab.assert_good_pack_usable();
}

#[test]
fn a_symlink_planted_at_the_target_snapshot_name_is_never_written_through() {
    let lab = Lab::new("hostile-snapshot-link");
    let root = lab.root();
    let outside = lab.dir.0.join("outside");
    // The attacker pre-creates `snapshots/victim-pack -> outside`.
    std::os::unix::fs::symlink(&outside, root.join("snapshots/victim-pack")).unwrap();
    let pack = lab
        .parts()
        .pack(|_| {}, |m| m.pack_id = "victim-pack".into());
    let path = lab.dir.0.join("attacks/victim.wpack");
    fs::write(&path, &pack).unwrap();
    let result = lab
        .library
        .install_pack(path.to_string_lossy().into_owned(), true);
    assert_eq!(result.err(), Some(LibraryError::AlreadyInstalled));
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    assert!(root.join("staging").read_dir().unwrap().next().is_none());
}

#[test]
fn reinstalling_over_an_installed_pack_id_never_replaces_the_installed_bytes() {
    let lab = Lab::new("hostile-overwrite");
    let evil = lab.parts().pack(
        |objects| objects.push(("extra.shard".into(), b"hostile replacement".to_vec())),
        |_| {},
    );
    lab.refuse(
        "overwrite_installed_pack",
        &evil,
        LibraryError::VerificationFailed,
    );
    // And a byte-identical reinstall is reported, not silently redone.
    let path = lab.dir.0.join("attacks/same.wpack");
    fs::write(&path, &lab.good).unwrap();
    let again = lab
        .library
        .install_pack(path.to_string_lossy().into_owned(), true);
    assert_eq!(again.err(), Some(LibraryError::AlreadyInstalled));
    lab.assert_good_pack_usable();
}

// ---------------------------------------------------------------------------
// 2. Decompression and resource bombs.

#[test]
fn manifest_length_field_bomb_is_refused_without_allocating() {
    let lab = Lab::new("hostile-length-bomb");
    for (name, length) in [
        ("transcript_length_u32_max", u32::MAX),
        ("transcript_length_over_cap", 64 * 1024 * 1024 + 1),
        ("transcript_length_zero", 0),
        ("transcript_length_past_end", 10_000_000),
    ] {
        let mut pack = lab.good.clone();
        pack[8..12].copy_from_slice(&length.to_be_bytes());
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::ResourceBudget | ContainerError::Io(_))
            ),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn string_field_length_bomb_is_refused() {
    let lab = Lab::new("hostile-field-bomb");
    let parts = lab.parts();
    for (name, length) in [
        ("field_length_2_pow_40", 1u64 << 40),
        ("field_length_u64_max", u64::MAX),
        ("field_length_just_over_cap", 4097),
    ] {
        let pack = parts.pack_with(
            |_| {},
            |_| {},
            |transcript, offsets| {
                transcript[offsets.pack_id_len..offsets.pack_id_len + 8]
                    .copy_from_slice(&length.to_be_bytes());
            },
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::ResourceBudget | ContainerError::Io(_))
            ),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn absurd_object_count_is_refused_before_reading_objects() {
    let lab = Lab::new("hostile-count-bomb");
    let parts = lab.parts();
    for (name, count) in [
        ("object_count_2_pow_40", 1u64 << 40),
        ("object_count_u64_max", u64::MAX),
        ("object_count_one_past_max", 1_000_001),
        ("object_count_zero", 0),
        ("object_count_larger_than_listed", 50),
    ] {
        let pack = parts.pack_with(
            |_| {},
            |_| {},
            |transcript, offsets| {
                transcript[offsets.object_count..offsets.object_count + 8]
                    .copy_from_slice(&count.to_be_bytes());
            },
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        store_error(name, &pack);
    }
}

#[test]
fn object_declared_far_beyond_its_real_size_is_refused() {
    let lab = Lab::new("hostile-size-bomb");
    let parts = lab.parts();
    // Tiny file, enormous declaration: a reader that trusted the length would
    // allocate or loop; ours reads at most the bytes that exist.
    for (name, declared) in [
        ("declared_1_tib_plus_one", (1u64 << 40) + 1),
        ("declared_u64_max", u64::MAX),
        ("declared_8_gib", 8u64 << 30),
        (
            "declared_one_byte_over",
            parts.object("articles.shard").len() as u64 + 1,
        ),
    ] {
        let pack = parts.pack(
            |_| {},
            |m| {
                m.objects
                    .iter_mut()
                    .find(|o| o.path == "articles.shard")
                    .unwrap()
                    .bytes = declared
            },
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        store_error(name, &pack);
    }
    // Many objects each at the per-object cap exceed the declared total.
    let pack = parts.pack(
        |objects| {
            for n in 0..5 {
                objects.push((format!("filler{n}"), vec![n as u8; 3]));
            }
        },
        |m| {
            for object in m
                .objects
                .iter_mut()
                .filter(|o| o.path.starts_with("filler"))
            {
                object.bytes = 1 << 40;
            }
        },
    );
    lab.refuse(
        "declared_total_over_4_tib",
        &pack,
        LibraryError::VerificationFailed,
    );
    let error = store_error("declared_total_over_4_tib", &pack);
    assert!(
        matches!(
            error,
            PackInstallError::Container(ContainerError::Manifest(
                ManifestError::ResourceBudget | ManifestError::ConflictingDigest
            ))
        ),
        "{error:?}"
    );
}

#[test]
fn spool_quota_stops_a_stream_larger_than_the_caller_allows() {
    let lab = Lab::new("hostile-spool-quota");
    let before = tree(&lab.dir.0);
    let quota = lab.good.len() as u64 - 1;
    let result = lab.library.install_pack_stream(
        Arc::new(VecSource(Mutex::new(Cursor::new(lab.good.clone())))),
        Arc::new(Approve),
        quota,
    );
    assert_eq!(result.err(), Some(LibraryError::VerificationFailed));
    // An endless stream is cut off at the quota instead of filling the disk.
    struct Endless;
    impl PackSource for Endless {
        fn read(&self, max_bytes: u32) -> Result<Vec<u8>, LibraryError> {
            Ok(vec![0xAA; max_bytes.min(4096) as usize])
        }
    }
    let result = lab
        .library
        .install_pack_stream(Arc::new(Endless), Arc::new(Approve), 1 << 20);
    assert_eq!(result.err(), Some(LibraryError::VerificationFailed));
    // A quota of zero or above the global cap is refused outright.
    for quota in [0, (8u64 << 30) + 1] {
        let result = lab
            .library
            .install_pack_stream(Arc::new(Endless), Arc::new(Approve), quota);
        assert_eq!(result.err(), Some(LibraryError::InvalidInput));
    }
    assert_eq!(tree(&lab.dir.0), before);
    lab.assert_good_pack_usable();
}

#[test]
fn a_failing_pack_source_is_a_typed_error_and_leaves_no_residue() {
    let lab = Lab::new("hostile-source-fails");
    let before = tree(&lab.dir.0);
    struct Dies(Mutex<usize>, Vec<u8>);
    impl PackSource for Dies {
        fn read(&self, max_bytes: u32) -> Result<Vec<u8>, LibraryError> {
            let mut at = self.0.lock().unwrap();
            if *at >= self.1.len() / 2 {
                return Err(LibraryError::SourceFailed);
            }
            let end = (*at + max_bytes.min(512) as usize).min(self.1.len() / 2);
            let chunk = self.1[*at..end].to_vec();
            *at = end;
            Ok(chunk)
        }
    }
    let result = lab.library.install_pack_stream(
        Arc::new(Dies(Mutex::new(0), lab.good.clone())),
        Arc::new(Approve),
        1 << 30,
    );
    assert!(result.is_err());
    assert_eq!(tree(&lab.dir.0), before);
    lab.assert_good_pack_usable();
}

// ---------------------------------------------------------------------------
// 3. Oversized objects, lying sizes, truncation, trailing bytes.

#[test]
fn declared_object_size_that_lies_is_refused_in_both_directions() {
    let lab = Lab::new("hostile-lying-sizes");
    let parts = lab.parts();
    let shard_len = parts.object("articles.shard").len() as u64;
    // Too small: the verifier consumes fewer bytes than exist, the digest of
    // that prefix cannot match, and the rest would be trailing data.
    // Too large: the stream ends early.
    for (name, declared) in [
        ("declared_size_too_small_by_one", shard_len - 1),
        ("declared_size_half", shard_len / 2),
        ("declared_size_too_large_by_one", shard_len + 1),
        ("declared_size_doubled", shard_len * 2),
    ] {
        let pack = parts.pack(
            |_| {},
            |m| {
                m.objects
                    .iter_mut()
                    .find(|o| o.path == "articles.shard")
                    .unwrap()
                    .bytes = declared
            },
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        store_error(name, &pack);
    }
    // A zero-length declaration for a real object.
    let pack = parts.pack(
        |_| {},
        |m| {
            m.objects
                .iter_mut()
                .find(|o| o.path == "articles.shard")
                .unwrap()
                .bytes = 0
        },
    );
    lab.refuse(
        "declared_size_zero",
        &pack,
        LibraryError::VerificationFailed,
    );
    store_error("declared_size_zero", &pack);
}

#[test]
fn truncated_pack_at_every_boundary_is_refused() {
    let lab = Lab::new("hostile-truncated");
    let length = u32::from_be_bytes(lab.good[8..12].try_into().unwrap()) as usize;
    let cuts = [
        0,
        1,
        7,
        8,
        11,
        12,
        12 + length / 2,
        12 + length - 1,
        12 + length,
        12 + length + 1,
        lab.good.len() / 2,
        lab.good.len() - 1,
    ];
    for cut in cuts {
        let pack = &lab.good[..cut];
        lab.refuse(
            &format!("truncated_at_{cut}"),
            pack,
            LibraryError::VerificationFailed,
        );
        store_error(&format!("truncated_at_{cut}"), pack);
    }
}

#[test]
fn trailing_bytes_after_the_last_object_are_refused() {
    let lab = Lab::new("hostile-trailing");
    let mut second = lab.good.clone();
    second.extend_from_slice(&lab.good);
    let mut zeros = lab.good.clone();
    zeros.extend_from_slice(&[0; 4096]);
    let mut one = lab.good.clone();
    one.push(0);
    let mut script = lab.good.clone();
    script.extend_from_slice(b"<script>alert(1)</script>");
    for (name, pack) in [
        ("trailing_one_byte", one),
        ("trailing_zero_page", zeros),
        ("trailing_second_pack", second),
        ("trailing_script_bytes", script),
    ] {
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::TrailingBytes)
            ),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn bad_envelope_header_is_refused() {
    let lab = Lab::new("hostile-header");
    let mut wrong_version = lab.good.clone();
    wrong_version[7] = b'2';
    let mut lowercase = lab.good.clone();
    lowercase[0] = b'f';
    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend_from_slice(&[0; 128]);
    let mut gzip = vec![0x1f, 0x8b, 8, 0];
    gzip.extend_from_slice(&[0; 128]);
    for (name, pack) in [
        ("empty_file", Vec::new()),
        ("magic_only", MAGIC.to_vec()),
        ("future_magic_version", wrong_version),
        ("lowercase_magic", lowercase),
        ("zip_file_renamed", zip),
        ("gzip_file_renamed", gzip),
        (
            "html_file_renamed",
            b"<!doctype html><script>1</script>".to_vec(),
        ),
        (
            "random_bytes",
            (0..4096u32).map(|n| (n * 31 + 7) as u8).collect(),
        ),
    ] {
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::InvalidHeader | ContainerError::Io(_))
            ),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn flipped_object_bytes_fail_the_digest_in_every_object() {
    let lab = Lab::new("hostile-bitflip");
    let parts = lab.parts();
    for (path, bytes) in &parts.objects {
        for at in [0, bytes.len() / 2, bytes.len() - 1] {
            let name = format!("bitflip_{path}_{at}");
            // Hashes in the manifest are those of the *good* bytes.
            let mut tampered = parts.honest();
            let start = tampered.len() - total_after(&parts, path) - bytes.len();
            tampered[start + at] ^= 0x01;
            lab.refuse(&name, &tampered, LibraryError::VerificationFailed);
            let error = store_error(&name, &tampered);
            assert!(
                matches!(
                    error,
                    PackInstallError::Container(ContainerError::ObjectDigest(_))
                ),
                "{name}: {error:?}"
            );
        }
    }
}

/// Payload bytes that follow `path` in canonical order.
fn total_after(parts: &Parts, path: &str) -> usize {
    parts
        .objects
        .iter()
        .skip_while(|(name, _)| name != path)
        .skip(1)
        .map(|(_, bytes)| bytes.len())
        .sum()
}

#[test]
fn objects_swapped_under_valid_names_fail_the_digest() {
    let lab = Lab::new("hostile-swapped-objects");
    let parts = lab.parts();
    // Same sizes, hashes of the originals, bytes exchanged.
    let a = parts.object("articles.shard").to_vec();
    let b = parts.object("catalog.sqlite").to_vec();
    let mut pack = parts.honest();
    let length = u32::from_be_bytes(pack[8..12].try_into().unwrap()) as usize;
    let payload = 12 + length;
    let mut swapped = Vec::new();
    for (path, _) in &parts.objects {
        swapped.extend_from_slice(if path == "articles.shard" { &b } else { &a });
    }
    pack.truncate(payload);
    pack.extend_from_slice(&swapped);
    lab.refuse("objects_swapped", &pack, LibraryError::VerificationFailed);
}

#[test]
fn oversized_image_object_is_refused() {
    let lab = Lab::new("hostile-oversized-image");
    // One byte over the per-image cap, with a valid PNG signature and a
    // matching hash, listed honestly in the index.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(MAX_PACK_IMAGE_BYTES as usize + 1, 0);
    let digest = hex(&png);
    let index = format!(
        "{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":{}}}]}}\n",
        png.len()
    );
    let pack = build_custom(
        &lab.dir.0.join("src"),
        "big-image",
        "<p>x</p>",
        &[("image/png", png)],
        Some(index.into_bytes()),
    );
    lab.refuse("oversized_image", &pack, LibraryError::MediaInvalid);
    let error = store_error("oversized_image", &pack);
    assert!(
        matches!(error, PackInstallError::Media(PackMediaError::InvalidIndex)),
        "{error:?}"
    );
}

// ---------------------------------------------------------------------------
// 5. Malformed manifests.

#[test]
fn malformed_transcripts_at_every_prefix_length_are_refused() {
    let lab = Lab::new("hostile-transcript-prefixes");
    let length = u32::from_be_bytes(lab.good[8..12].try_into().unwrap()) as usize;
    let transcript = &lab.good[12..12 + length];
    let payload = &lab.good[12 + length..];
    let root = test_dir("hostile-prefix-root");
    let root_path = fs::canonicalize(&root.0).unwrap();
    // Every proper prefix of the manifest is "a manifest with missing
    // fields": none may install, and none may panic.
    for cut in 0..length {
        let pack = frame(&transcript[..cut], payload);
        let result = install_custom_pack(&root_path, &mut Cursor::new(&pack), 1 << 30, |_| true);
        assert!(result.is_err(), "prefix {cut} installed");
    }
    assert_root_clean("manifest_prefixes", &root_path);
}

#[test]
fn flipping_any_manifest_byte_never_panics_or_escapes() {
    let lab = Lab::new("hostile-transcript-flips");
    let length = u32::from_be_bytes(lab.good[8..12].try_into().unwrap()) as usize;
    let root = test_dir("hostile-flip-root");
    let root_path = fs::canonicalize(&root.0).unwrap();
    let mut installed_variants = 0;
    for at in 12..12 + length {
        for bit in [0x01, 0x80] {
            let mut pack = lab.good.clone();
            pack[at] ^= bit;
            // A flip inside a free-text identity can yield a different but
            // valid pack (for example another pack ID); anything else must be
            // refused. Either way: no panic, and nothing outside the root.
            if let Ok(installed) =
                install_custom_pack(&root_path, &mut Cursor::new(&pack), 1 << 30, |_| true)
            {
                installed_variants += 1;
                assert!(installed.path.starts_with(root_path.join("snapshots")));
                remove_installed_snapshot(&root_path, &installed.snapshot_id).unwrap();
            }
        }
    }
    assert!(
        installed_variants < length,
        "implausibly many manifest flips installed ({installed_variants})"
    );
    assert_root_clean("manifest_flips", &root_path);
    assert_eq!(tree(&lab.dir.0.join("outside")).len(), 0);
}

#[test]
fn manifest_schema_version_and_prefix_attacks_are_refused() {
    let lab = Lab::new("hostile-versions");
    let parts = lab.parts();
    for (name, version) in [
        ("schema_version_zero", 0u32),
        ("schema_version_one_old", 1),
        ("schema_version_three_future", 3),
        ("schema_version_u32_max", u32::MAX),
    ] {
        let pack = parts.pack(|_| {}, |m| m.version = version);
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        store_error(name, &pack);
    }
    let mut prefix = parts.honest();
    prefix[12 + TRANSCRIPT_PREFIX.len() - 2] = b'3';
    lab.refuse(
        "transcript_prefix_v3",
        &prefix,
        LibraryError::VerificationFailed,
    );
    let mut unknown_origin = parts.honest();
    // Origin kind tag is the byte after the three identity fields.
    let at = 12
        + TRANSCRIPT_PREFIX.len()
        + 4
        + 3 * 8
        + ["good-pack", "enwiki", "20261101"]
            .iter()
            .map(|s| s.len())
            .sum::<usize>();
    assert_eq!(unknown_origin[at], 2, "located the origin tag");
    unknown_origin[at] = 9;
    lab.refuse(
        "unknown_origin_kind",
        &unknown_origin,
        LibraryError::VerificationFailed,
    );
}

#[test]
fn noncanonical_manifest_encodings_are_refused() {
    let lab = Lab::new("hostile-noncanonical");
    let parts = lab.parts();
    // Reordered objects: the same manifest in a different byte spelling must
    // not verify (it would give two identities to one pack).
    let mut reordered = parts.manifest.clone();
    reordered.objects.sort_by(|a, b| b.path.cmp(&a.path));
    let (transcript, _) = encode_transcript(&reordered);
    let payload: Vec<u8> = parts.objects.iter().flat_map(|(_, b)| b.clone()).collect();
    lab.refuse(
        "objects_out_of_order",
        &frame(&transcript, &payload),
        LibraryError::VerificationFailed,
    );
    // Uppercase digests.
    let mut upper = parts.manifest.clone();
    upper.objects.sort_by(|a, b| a.path.cmp(&b.path));
    for object in &mut upper.objects {
        object.sha256 = object.sha256.to_uppercase();
    }
    let (transcript, _) = encode_transcript(&upper);
    lab.refuse(
        "uppercase_digests",
        &frame(&transcript, &payload),
        LibraryError::VerificationFailed,
    );
    // Junk appended inside the manifest region.
    let (mut transcript, _) = encode_transcript(&{
        let mut m = parts.manifest.clone();
        m.objects.sort_by(|a, b| a.path.cmp(&b.path));
        m
    });
    transcript.extend_from_slice(b"\0extra");
    lab.refuse(
        "junk_inside_manifest_region",
        &frame(&transcript, &payload),
        LibraryError::VerificationFailed,
    );
}

#[test]
fn manifest_object_list_lies_are_refused() {
    let lab = Lab::new("hostile-object-lists");
    let parts = lab.parts();
    // Bad or weak digests.
    for (name, digest) in [
        ("digest_too_short", "ab".to_owned()),
        ("digest_not_hex", "z".repeat(64)),
        ("digest_wrong_object", hex(b"something else")),
        ("digest_all_zero", "0".repeat(64)),
        ("digest_empty", String::new()),
    ] {
        let pack = parts.pack(
            |_| {},
            |m| {
                m.objects
                    .iter_mut()
                    .find(|o| o.path == "articles.shard")
                    .unwrap()
                    .sha256 = digest
            },
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        store_error(name, &pack);
    }
    // One digest used for two objects with different sizes.
    let pack = parts.pack(
        |objects| objects.push(("copy.shard".into(), b"short".to_vec())),
        |m| {
            let digest = m
                .objects
                .iter()
                .find(|o| o.path == "articles.shard")
                .unwrap()
                .sha256
                .clone();
            m.objects
                .iter_mut()
                .find(|o| o.path == "copy.shard")
                .unwrap()
                .sha256 = digest;
        },
    );
    lab.refuse(
        "conflicting_digest_sizes",
        &pack,
        LibraryError::VerificationFailed,
    );
    let error = store_error("conflicting_digest_sizes", &pack);
    assert!(
        matches!(
            error,
            PackInstallError::Container(ContainerError::Manifest(ManifestError::ConflictingDigest))
        ),
        "{error:?}"
    );
    // The same path listed twice.
    let pack = parts.pack(
        |objects| {
            objects.push((
                "articles.shard".into(),
                parts.object("articles.shard").to_vec(),
            ))
        },
        |_| {},
    );
    lab.refuse(
        "duplicate_object_path",
        &pack,
        LibraryError::VerificationFailed,
    );
    // An undeclared object is simply not in the pack; a declared-but-absent
    // one is a truncated stream.
    let pack = parts.pack(
        |_| {},
        |m| {
            m.objects.push(Object {
                path: "zzz-missing.shard".into(),
                sha256: hex(b"never sent"),
                bytes: 10,
            })
        },
    );
    lab.refuse(
        "declared_object_absent",
        &pack,
        LibraryError::VerificationFailed,
    );
}

#[test]
fn manifest_metadata_lies_are_refused() {
    let lab = Lab::new("hostile-metadata");
    let parts = lab.parts();
    let refused =
        |name: &str, edit: &dyn Fn(&mut Manifest), want: fn(&PackInstallError) -> bool| {
            let pack = parts.pack(|_| {}, |m| edit(m));
            lab.refuse(name, &pack, LibraryError::VerificationFailed);
            let error = store_error(name, &pack);
            assert!(want(&error), "{name}: {error:?}");
        };
    let invalid_metadata = |e: &PackInstallError| {
        matches!(
            e,
            PackInstallError::Container(ContainerError::Manifest(
                ManifestError::InvalidPackMetadata | ManifestError::UnsafeMetadata
            ))
        )
    };
    let budget = |e: &PackInstallError| {
        matches!(
            e,
            PackInstallError::Container(ContainerError::Manifest(ManifestError::ResourceBudget))
        )
    };
    refused(
        "source_hash_uppercase",
        &|m| m.metadata.source.manifest_sha256 = "A".repeat(64),
        invalid_metadata,
    );
    refused(
        "source_hash_short",
        &|m| m.metadata.source.manifest_sha256 = "ab".into(),
        invalid_metadata,
    );
    refused(
        "definition_hash_bad",
        &|m| m.metadata.definition_sha256 = "x".repeat(64),
        invalid_metadata,
    );
    refused(
        "license_spdx_empty",
        &|m| m.metadata.license.text_spdx = String::new(),
        invalid_metadata,
    );
    refused(
        "license_spdx_script",
        &|m| m.metadata.license.text_spdx = "<script>".into(),
        invalid_metadata,
    );
    refused(
        "license_spdx_overlong",
        &|m| m.metadata.license.text_spdx = "a".repeat(65),
        invalid_metadata,
    );
    refused(
        "attribution_object_undeclared",
        &|m| m.metadata.license.attribution_object = Some("nowhere.txt".into()),
        invalid_metadata,
    );
    refused(
        "attribution_traversal",
        &|m| m.metadata.license.attribution_object = Some("../../etc/passwd".into()),
        invalid_metadata,
    );
    refused(
        "media_count_without_attribution",
        &|m| m.metadata.counts.media = 3,
        invalid_metadata,
    );
    refused(
        "index_points_at_undeclared_object",
        &|m| m.metadata.indexes[0].path = "../catalog.sqlite".into(),
        invalid_metadata,
    );
    refused(
        "index_schema_zero",
        &|m| m.metadata.indexes[0].schema = 0,
        invalid_metadata,
    );
    refused(
        "index_declared_twice",
        &|m| {
            let first = m.metadata.indexes[0].clone();
            m.metadata.indexes.push(first)
        },
        invalid_metadata,
    );
    refused(
        "articles_count_absurd",
        &|m| m.metadata.counts.articles = u64::MAX,
        budget,
    );
    refused(
        "redirects_count_absurd",
        &|m| m.metadata.counts.redirects = 1_000_000_000_001,
        budget,
    );
    refused(
        "media_count_absurd",
        &|m| {
            m.metadata.counts.media = 1_000_001;
            m.metadata.license.attribution_object = Some("catalog.sqlite".into());
            m.metadata.indexes.clear()
        },
        |_| true,
    );
    // Valid shape, wrong facts: the installer recounts the verified catalog.
    refused(
        "articles_count_inflated",
        &|m| m.metadata.counts.articles = 99,
        |e| matches!(e, PackInstallError::CountMismatch { .. }),
    );
    refused(
        "articles_count_deflated",
        &|m| m.metadata.counts.articles = 1,
        |e| matches!(e, PackInstallError::CountMismatch { .. }),
    );
    refused(
        "redirects_count_inflated",
        &|m| m.metadata.counts.redirects = 7,
        |e| matches!(e, PackInstallError::CountMismatch { .. }),
    );
    // Layout lies.
    refused(
        "no_catalog_declared",
        &|m| m.metadata.indexes.clear(),
        |e| matches!(e, PackInstallError::MissingCatalog),
    );
    refused(
        "catalog_schema_from_the_future",
        &|m| m.metadata.indexes[0].schema = 99,
        |e| matches!(e, PackInstallError::UnsupportedObject(_)),
    );
    refused(
        "fts_role_on_the_catalog",
        &|m| m.metadata.indexes[0].role = IndexRole::Fts5,
        |e| {
            matches!(
                e,
                PackInstallError::UnsupportedObject(_) | PackInstallError::MissingCatalog
            )
        },
    );
}

#[test]
fn an_official_origin_claim_without_a_signature_is_refused() {
    let lab = Lab::new("hostile-unsigned-official");
    for publisher in [
        "foundation-wikipedia",
        "someone-else",
        "Foundation-Wikipedia",
    ] {
        let pack = lab.parts().pack(
            |_| {},
            |m| {
                m.origin = Origin::Official {
                    publisher: publisher.into(),
                }
            },
        );
        lab.refuse(
            "unsigned_official_claim",
            &pack,
            LibraryError::UnsignedOfficialPack,
        );
        let error = store_error("unsigned_official_claim", &pack);
        assert!(
            matches!(error, PackInstallError::UnsignedOfficialPack),
            "{error:?}"
        );
    }
}

#[test]
fn garbage_catalog_and_shards_with_valid_hashes_are_refused() {
    let lab = Lab::new("hostile-garbage-objects");
    let parts = lab.parts();
    let mut huge_frame = vec![0xff; 16];
    huge_frame.resize(parts.object("articles.shard").len(), 0);
    for (name, path, bytes) in [
        (
            "catalog_is_not_sqlite",
            "catalog.sqlite",
            b"not a database at all".to_vec(),
        ),
        (
            "catalog_is_empty_sqlite_header",
            "catalog.sqlite",
            b"SQLite format 3\0".to_vec(),
        ),
        (
            "catalog_is_the_shard",
            "catalog.sqlite",
            parts.object("articles.shard").to_vec(),
        ),
        (
            "shard_is_random",
            "articles.shard",
            vec![0x5a; parts.object("articles.shard").len()],
        ),
        (
            "shard_declares_a_four_gib_frame",
            "articles.shard",
            huge_frame.clone(),
        ),
        ("shard_is_one_byte", "articles.shard", vec![1]),
        (
            "shard_is_html",
            "articles.shard",
            b"<html><script>alert(1)</script></html>".to_vec(),
        ),
    ] {
        let pack = parts.pack(
            |objects| {
                objects.iter_mut().find(|(p, _)| p == path).unwrap().1 = bytes;
            },
            |_| {},
        );
        lab.refuse(name, &pack, LibraryError::VerificationFailed);
        store_error(name, &pack);
    }
}

// ---------------------------------------------------------------------------
// 4. MIME spoofing.

const SVG_BODY: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>";

fn image_pack(lab: &Lab, id: &str, mime: &str, bytes: &[u8]) -> Vec<u8> {
    build_custom(
        &lab.dir.0.join("src"),
        id,
        "<p>x</p>",
        &[(mime, bytes.to_vec())],
        None,
    )
}

/// The support builder's SVG pack with `svg` as the figure, from a fresh tree.
fn svg_pack(lab: &Lab, id: &str, svg: &[u8]) -> Vec<u8> {
    let src = lab.dir.0.join("src");
    let _ = fs::remove_dir_all(src.join(format!("{id}-tree")));
    fs::read(build_image_pack_with_svg(&src, id, svg)).unwrap()
}

#[test]
fn a_png_declared_as_svg_is_refused() {
    let lab = Lab::new("hostile-png-as-svg");
    let pack = image_pack(&lab, "png-as-svg", "image/svg+xml", &RED_PNG);
    lab.refuse("png_declared_svg", &pack, LibraryError::MediaInvalid);
    let error = store_error("png_declared_svg", &pack);
    assert!(
        matches!(error, PackInstallError::Media(PackMediaError::UnsafeSvg(_))),
        "{error:?}"
    );
}

#[test]
fn an_svg_declared_as_a_raster_image_is_refused() {
    let lab = Lab::new("hostile-svg-as-png");
    for mime in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        let name = format!("svg_declared_{}", mime.replace('/', "_"));
        // Even a perfectly clean SVG is not a PNG.
        for bytes in [blue_svg(), SVG_BODY.to_vec()] {
            let pack = image_pack(&lab, "svg-as-raster", mime, &bytes);
            lab.refuse(&name, &pack, LibraryError::MediaInvalid);
            let error = store_error(&name, &pack);
            assert!(
                matches!(error, PackInstallError::Media(_)),
                "{name}: {error:?}"
            );
        }
    }
}

#[test]
fn html_and_script_bytes_declared_as_images_are_refused() {
    let lab = Lab::new("hostile-html-as-image");
    let payloads: [(&str, &[u8]); 6] = [
        (
            "html_page",
            b"<!doctype html><html><script>fetch('//evil.example')</script></html>",
        ),
        ("script_tag", b"<script>alert(document.cookie)</script>"),
        ("javascript_source", b"javascript:alert(1)//"),
        (
            "svg_with_script",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
        ),
        ("shell_script", b"#!/bin/sh\nrm -rf ~\n"),
        ("elf_binary", b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0"),
    ];
    for mime in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        for (what, bytes) in payloads {
            let name = format!("{what}_declared_{}", mime.replace('/', "_"));
            let pack = image_pack(&lab, "html-as-image", mime, bytes);
            lab.refuse(&name, &pack, LibraryError::MediaInvalid);
            let error = store_error(&name, &pack);
            assert!(
                matches!(error, PackInstallError::Media(_)),
                "{name}: {error:?}"
            );
        }
    }
}

#[test]
fn a_raster_declared_as_a_different_raster_is_refused() {
    let lab = Lab::new("hostile-raster-mismatch");
    for mime in ["image/jpeg", "image/gif", "image/webp"] {
        let name = format!("png_declared_{}", mime.replace('/', "_"));
        let pack = image_pack(&lab, "png-as-other", mime, &RED_PNG);
        lab.refuse(&name, &pack, LibraryError::MediaInvalid);
    }
}

/// A 33-byte file whose header claims a canvas needing tens of gigabytes to
/// decode: the image analogue of a decompression bomb.
#[test]
fn a_tiny_png_or_gif_declaring_a_gigapixel_canvas_is_refused() {
    let lab = Lab::new("hostile-pixel-bomb");
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&100_000u32.to_be_bytes());
    png.extend_from_slice(&100_000u32.to_be_bytes());
    png.extend_from_slice(b"\x08\x02\0\0\0");
    let mut gif = b"GIF89a\xff\xff\xff\xff\0\0\0".to_vec();
    gif.extend_from_slice(b";");
    for (name, mime, bytes) in [
        ("png_100k_by_100k", "image/png", png),
        ("gif_65535_by_65535", "image/gif", gif),
    ] {
        let pack = image_pack(&lab, "pixel-bomb", mime, &bytes);
        lab.refuse(name, &pack, LibraryError::MediaInvalid);
        let error = store_error(name, &pack);
        assert!(
            matches!(
                error,
                PackInstallError::Media(PackMediaError::SpoofedImage(_))
            ),
            "{name}: {error:?}"
        );
    }
}

/// A raster with a valid header and HTML/script after it is a valid image
/// to a decoder and a page to a careless server. Contract: installed or
/// refused, it is only ever handed back as its declared image type, and the
/// readers serve that with `nosniff` and a sandbox CSP (checked on Android
/// by `PackImagesUiTest`, on desktop by `offline_packs` tests), so it can
/// never run as a document.
#[test]
fn raster_polyglots_with_valid_headers_are_only_ever_served_as_images() {
    let lab = Lab::new("hostile-raster-polyglot");
    let payload: &[u8] = b"<!doctype html><html><body><script>alert(document.domain)</script>\
<iframe src=\"https://evil.example/\"></iframe></body></html>";
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0".to_vec();
    png.extend_from_slice(payload);
    let mut gif = b"GIF89a\x01\0\x01\0\0\0\0".to_vec();
    gif.extend_from_slice(payload);
    let mut jpeg = b"\xff\xd8\xff\xe0\0\x10JFIF\0".to_vec();
    jpeg.extend_from_slice(payload);
    for (name, mime, bytes) in [
        ("png_html", "image/png", png),
        ("gif_html", "image/gif", gif),
        ("jpeg_html", "image/jpeg", jpeg),
    ] {
        let path = lab.dir.0.join(format!("attacks/{name}.wpack"));
        fs::write(&path, image_pack(&lab, name, mime, &bytes)).unwrap();
        match lab
            .library
            .install_pack(path.to_string_lossy().into_owned(), true)
        {
            Ok(_) => {
                lab.library.activate(name.into()).unwrap();
                let digest = hex(&bytes);
                let image = lab
                    .library
                    .read_image(digest.clone())
                    .unwrap()
                    .expect("installed image is readable");
                assert_eq!(image.mime, mime, "{name}: served only as its declared type");
                assert_eq!(image.bytes, bytes, "{name}");
                let page = lab
                    .library
                    .open_reader_page("enwiki".into(), 5, false)
                    .unwrap()
                    .unwrap();
                assert!(!page.document.contains("alert(document.domain)"), "{name}");
                lab.library.activate("good-pack".into()).unwrap();
                lab.library.remove_pack(name.into()).unwrap();
            }
            Err(error) => assert_eq!(error, LibraryError::MediaInvalid, "{name}"),
        }
    }
    for rule in ["default-src 'none'", "sandbox"] {
        assert!(PACK_IMAGE_CSP.contains(rule));
    }
    lab.assert_good_pack_usable();
}

#[test]
fn a_polyglot_png_script_declared_as_svg_is_refused() {
    let lab = Lab::new("hostile-polyglot");
    // PNG signature followed by markup: the PNG-or-HTML polyglot.
    let mut polyglot = b"\x89PNG\r\n\x1a\n".to_vec();
    polyglot.extend_from_slice(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
    );
    let pack = image_pack(&lab, "polyglot", "image/svg+xml", &polyglot);
    lab.refuse("png_svg_polyglot", &pack, LibraryError::MediaInvalid);
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>1</script></svg>");
    let pack = image_pack(&lab, "polyglot2", "image/svg+xml", &gif);
    lab.refuse("gif_svg_polyglot", &pack, LibraryError::MediaInvalid);
}

#[test]
fn undeclared_or_unknown_image_types_are_refused() {
    let lab = Lab::new("hostile-mime-types");
    let digest = hex(&RED_PNG);
    for mime in [
        "text/html",
        "application/javascript",
        "application/x-msdownload",
        "image/svg+xml; charset=utf-8",
        "IMAGE/PNG",
        "image/png ",
        "image/x-icon",
        "image/*",
        "",
    ] {
        let index = format!(
            "{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"{mime}\",\"bytes\":{}}}]}}\n",
            RED_PNG.len()
        );
        let pack = build_custom(
            &lab.dir.0.join("src"),
            "bad-mime",
            "<p>x</p>",
            &[("image/png", RED_PNG.to_vec())],
            Some(index.into_bytes()),
        );
        lab.refuse(&format!("mime_{mime}"), &pack, LibraryError::MediaInvalid);
    }
}

// ---------------------------------------------------------------------------
// 5 (cont.). Malformed media index JSON.

#[test]
fn malformed_media_index_json_is_refused() {
    let lab = Lab::new("hostile-media-index");
    let digest = hex(&RED_PNG);
    let len = RED_PNG.len();
    let entry = format!("{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":{len}}}");
    let indexes: Vec<(&str, String)> = vec![
        ("index_not_json", "not json at all".into()),
        ("index_truncated", format!("{{\"version\":1,\"images\":[{entry}")),
        ("index_duplicate_version_key", format!("{{\"version\":1,\"version\":1,\"images\":[{entry}]}}\n")),
        ("index_duplicate_images_key", format!("{{\"version\":1,\"images\":[],\"images\":[{entry}]}}\n")),
        ("index_duplicate_entry_key", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":{len}}}]}}\n")),
        ("index_version_two", format!("{{\"version\":2,\"images\":[{entry}]}}\n")),
        ("index_version_missing", format!("{{\"images\":[{entry}]}}\n")),
        ("index_images_missing", "{\"version\":1}\n".into()),
        ("index_images_is_object", "{\"version\":1,\"images\":{}}\n".into()),
        ("index_entry_missing_mime", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"bytes\":{len}}}]}}\n")),
        ("index_entry_bytes_is_string", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":\"{len}\"}}]}}\n")),
        ("index_entry_bytes_negative", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":-1}}]}}\n")),
        ("index_entry_bytes_float", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":97.0}}]}}\n")),
        ("index_entry_bytes_zero", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":0}}]}}\n")),
        ("index_entry_bytes_u64_max", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":18446744073709551615}}]}}\n")),
        ("index_entry_bytes_lies_small", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":{}}}]}}\n", len - 1)),
        ("index_entry_bytes_lies_large", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{digest}\",\"mime\":\"image/png\",\"bytes\":{}}}]}}\n", len + 1)),
        ("index_digest_uppercase", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{}\",\"mime\":\"image/png\",\"bytes\":{len}}}]}}\n", digest.to_uppercase())),
        ("index_digest_is_a_path", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"../../etc/passwd\",\"mime\":\"image/png\",\"bytes\":{len}}}]}}\n")),
        ("index_digest_other_image", format!("{{\"version\":1,\"images\":[{{\"sha256\":\"{}\",\"mime\":\"image/png\",\"bytes\":{len}}}]}}\n", "e".repeat(64))),
        ("index_extra_whitespace", format!("{{ \"version\":1,\"images\":[{entry}]}}\n")),
        ("index_no_trailing_newline", format!("{{\"version\":1,\"images\":[{entry}]}}")),
        ("index_unknown_extra_field", format!("{{\"version\":1,\"images\":[{entry}],\"extra\":1}}\n")),
        ("index_lists_nothing_but_pack_has_image", "{\"version\":1,\"images\":[]}\n".into()),
        ("index_lists_image_twice", format!("{{\"version\":1,\"images\":[{entry},{entry}]}}\n")),
        ("index_is_deeply_nested", format!("{}{}", "[".repeat(5000), "]".repeat(5000))),
        ("index_nul_bytes", "{\"version\":1,\"images\":[]}\0\n".into()),
    ];
    for (name, index) in indexes {
        let pack = build_custom(
            &lab.dir.0.join("src"),
            "bad-index",
            "<p>x</p>",
            &[("image/png", RED_PNG.to_vec())],
            Some(index.into_bytes()),
        );
        lab.refuse(name, &pack, LibraryError::MediaInvalid);
        let error = store_error(name, &pack);
        assert!(
            matches!(error, PackInstallError::Media(_)),
            "{name}: {error:?}"
        );
    }
}

#[test]
fn media_objects_that_do_not_match_the_articles_and_index_are_refused() {
    let lab = Lab::new("hostile-media-mismatch");
    let good = build_custom(
        &lab.dir.0.join("src"),
        "media-good",
        "<p>x</p>",
        &[("image/png", RED_PNG.to_vec())],
        None,
    );
    // The control installs through a scratch root: the harness is not
    // refusing everything.
    let control = test_dir("hostile-media-control");
    let control_root = fs::canonicalize(&control.0).unwrap();
    install_custom_pack(
        &control_root,
        &mut Cursor::new(good.clone()),
        1 << 30,
        |_| true,
    )
    .expect("a well-formed image pack installs");

    let parts = Parts::of(&good);
    let red = media_object_path(&hex(&RED_PNG));
    // An extra image no article uses.
    let stray = b"\x89PNG\r\n\x1a\nstray".to_vec();
    let pack = parts.pack(
        |objects| objects.push((media_object_path(&hex(&stray)), stray.clone())),
        |_| {},
    );
    lab.refuse("unlisted_media_object", &pack, LibraryError::MediaInvalid);
    // The listed image's object removed.
    let pack = parts.pack(|objects| objects.retain(|(p, _)| *p != red), |_| {});
    lab.refuse("listed_image_missing", &pack, LibraryError::MediaInvalid);
    // The image object holding other bytes (hash recomputed honestly, so the
    // envelope verifies; the name no longer matches the content).
    let pack = parts.pack(
        |objects| {
            objects.iter_mut().find(|(p, _)| *p == red).unwrap().1 =
                b"\x89PNG\r\n\x1a\nother".to_vec()
        },
        |_| {},
    );
    lab.refuse(
        "image_bytes_differ_from_name",
        &pack,
        LibraryError::MediaInvalid,
    );
    // Media count that disagrees with the index.
    let pack = parts.pack(|_| {}, |m| m.metadata.counts.media = 2);
    lab.refuse("media_count_mismatch", &pack, LibraryError::MediaInvalid);
    // The index dropped from the manifest while images remain.
    let pack = parts.pack(
        |objects| objects.retain(|(p, _)| p != MEDIA_INDEX_OBJECT),
        |m| {
            m.metadata.license.attribution_object = None;
            m.metadata.counts.media = 0;
        },
    );
    lab.refuse("image_without_index", &pack, LibraryError::MediaInvalid);
    // A text-only pack that smuggles media objects.
    let text_only = Parts::of(&lab.good);
    let pack = text_only.pack(
        |objects| objects.push((media_object_path(&hex(&RED_PNG)), RED_PNG.to_vec())),
        |_| {},
    );
    lab.refuse("media_in_text_only_pack", &pack, LibraryError::MediaInvalid);
    let error = store_error("media_in_text_only_pack", &pack);
    assert!(
        matches!(
            error,
            PackInstallError::Media(PackMediaError::UndeclaredMedia)
        ),
        "{error:?}"
    );
}

// ---------------------------------------------------------------------------
// 6. Unsafe SVG and HTML inside packs whose hashes are all valid.

#[test]
fn hostile_svg_in_a_pack_with_valid_hashes_is_refused() {
    let lab = Lab::new("hostile-svg");
    let ns = "xmlns=\"http://www.w3.org/2000/svg\"";
    let xlink = "xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
    let entity_bomb = {
        let mut dtd = String::from("<!DOCTYPE svg [<!ENTITY a0 \"lol\">");
        for n in 1..10 {
            let prev = format!("&a{};", n - 1).repeat(10);
            dtd.push_str(&format!("<!ENTITY a{n} \"{prev}\">"));
        }
        dtd.push_str("]>");
        format!("{dtd}<svg {ns}><text>&a9;</text></svg>")
    };
    let nested = format!(
        "<svg {ns}>{}{}</svg>",
        "<g>".repeat(5000),
        "</g>".repeat(5000)
    );
    let many_nodes = format!("<svg {ns}>{}</svg>", "<g/>".repeat(300_000));
    let oversized = format!(
        "<svg {ns}><text>{}</text></svg>",
        "A".repeat(1024 * 1024 + 1)
    );
    let attacks: Vec<(&str, String)> = vec![
        ("script_element", format!("<svg {ns}><script>alert(1)</script></svg>")),
        ("script_with_cdata", format!("<svg {ns}><script><![CDATA[alert(1)]]></script></svg>")),
        ("onload_handler", format!("<svg {ns} onload=\"alert(1)\"/>")),
        ("onclick_handler", format!("<svg {ns}><rect onclick=\"alert(1)\" width=\"1\" height=\"1\"/></svg>")),
        ("onerror_in_image", format!("<svg {ns}><image href=\"x\" onerror=\"alert(1)\"/></svg>")),
        ("foreign_object_html", format!("<svg {ns}><foreignObject><body xmlns=\"http://www.w3.org/1999/xhtml\"><script>alert(1)</script></body></foreignObject></svg>")),
        ("foreign_object_iframe", format!("<svg {ns}><foreignObject><iframe xmlns=\"http://www.w3.org/1999/xhtml\" src=\"https://evil.example\"/></foreignObject></svg>")),
        ("external_image_href", format!("<svg {ns}><image href=\"https://evil.example/x.png\"/></svg>")),
        ("external_image_xlink", format!("<svg {ns} {xlink}><image xlink:href=\"//evil.example/x.png\"/></svg>")),
        ("data_uri_image", format!("<svg {ns}><image href=\"data:image/svg+xml;base64,PHN2Zy8+\"/></svg>")),
        ("use_external_reference", format!("<svg {ns} {xlink}><use xlink:href=\"https://evil.example/a.svg#x\"/></svg>")),
        ("use_relative_file", format!("<svg {ns}><use href=\"other.svg#x\"/></svg>")),
        ("use_file_scheme", format!("<svg {ns}><use href=\"file:///etc/passwd\"/></svg>")),
        ("anchor_javascript", format!("<svg {ns}><a href=\"javascript:alert(1)\"><rect width=\"1\" height=\"1\"/></a></svg>")),
        ("css_import", format!("<svg {ns}><style>@import url(https://evil.example/x.css);</style></svg>")),
        ("css_url_external", format!("<svg {ns}><style>rect{{fill:url(https://evil.example/x)}}</style></svg>")),
        ("style_attribute_url", format!("<svg {ns}><rect style=\"fill:url(http://evil.example/x)\" width=\"1\" height=\"1\"/></svg>")),
        ("css_expression_escape", format!("<svg {ns}><style>rect{{fill:\\75rl(http://evil.example)}}</style></svg>")),
        ("fill_url_external", format!("<svg {ns}><rect fill=\"url(https://evil.example/x#a)\" width=\"1\" height=\"1\"/></svg>")),
        ("animate_href_javascript", format!("<svg {ns}><a><animate attributeName=\"href\" values=\"javascript:alert(1)\"/><text>x</text></a></svg>")),
        ("set_onbegin", format!("<svg {ns}><set attributeName=\"x\" to=\"1\" onbegin=\"alert(1)\"/></svg>")),
        ("xml_stylesheet_instruction", format!("<?xml-stylesheet href=\"https://evil.example/x.css\"?><svg {ns}/>")),
        ("doctype_external_entity", format!("<!DOCTYPE svg [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><svg {ns}><text>&x;</text></svg>")),
        ("doctype_external_dtd", format!("<!DOCTYPE svg SYSTEM \"https://evil.example/svg.dtd\"><svg {ns}/>")),
        ("entity_expansion_bomb", entity_bomb),
        ("html_namespace_script", format!("<svg {ns}><script xmlns=\"http://www.w3.org/1999/xhtml\">alert(1)</script></svg>")),
        ("not_an_svg_root", "<html xmlns=\"http://www.w3.org/1999/xhtml\"><script>alert(1)</script></html>".to_string()),
        ("embed_element", format!("<svg {ns}><embed src=\"https://evil.example\"/></svg>")),
        ("object_element", format!("<svg {ns}><object data=\"https://evil.example\"/></svg>")),
        ("audio_element", format!("<svg {ns}><audio src=\"https://evil.example/a.mp3\"/></svg>")),
        ("comment_with_payload", format!("<svg {ns}><!-- <script>alert(1)</script> --></svg>")),
        ("not_xml", "<svg ".into()),
        ("invalid_utf8_marker", "\u{fffd}\u{fffd}".into()),
        ("deeply_nested", nested),
        ("node_count_bomb", many_nodes),
        ("over_one_mebibyte", oversized),
    ];
    for (name, svg) in &attacks {
        let pack = svg_pack(&lab, "svg-attack", svg.as_bytes());
        lab.refuse(name, &pack, LibraryError::MediaInvalid);
        let error = store_error(name, &pack);
        assert!(
            matches!(error, PackInstallError::Media(PackMediaError::UnsafeSvg(_))),
            "{name}: {error:?}"
        );
    }
    // Invalid UTF-8 and a BOM-prefixed document, as raw bytes.
    for (name, bytes) in [
        (
            "svg_invalid_utf8",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\">\xff\xfe</svg>".to_vec(),
        ),
        ("svg_utf16_bom", b"\xff\xfe<\0s\0v\0g\0/\0>\0".to_vec()),
    ] {
        let pack = svg_pack(&lab, "svg-bytes", &bytes);
        lab.refuse(name, &pack, LibraryError::MediaInvalid);
    }
}

/// Markup that is dangerous if it ever reached a WebView unsanitised.
const HOSTILE_ARTICLE: &str = concat!(
    "<article><h1>Hostile</h1><p>Harmless sentence.</p>",
    "<script>alert('script-element')</script>",
    "<p onclick=\"alert('onclick')\" onmouseover=\"alert(1)\">Handler paragraph.</p>",
    "<a href=\"javascript:alert('js-link')\">js link</a>",
    "<a href=\" JaVa\tScRiPt:alert(1)\">obfuscated js link</a>",
    "<a href=\"data:text/html,<script>alert(1)</script>\">data link</a>",
    "<a href=\"vbscript:msgbox(1)\">vbs link</a>",
    "<a href=\"file:///etc/passwd\">file link</a>",
    "<a href=\"intent://scan/#Intent;scheme=zxing;end\">intent link</a>",
    "<img src=\"https://tracker.example/pixel.png\" onerror=\"alert('img')\">",
    "<img src=\"x\" srcset=\"https://tracker.example/a.png 1x\">",
    "<iframe src=\"https://evil.example/frame\"></iframe>",
    "<object data=\"https://evil.example/o\"></object>",
    "<embed src=\"https://evil.example/e\">",
    "<form action=\"https://evil.example/post\"><input name=\"pw\"><button>go</button></form>",
    "<base href=\"https://evil.example/\">",
    "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
    "<link rel=\"stylesheet\" href=\"https://evil.example/x.css\">",
    "<style>body{background:url(https://evil.example/bg.png)}</style>",
    "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert('svg-script')</script>",
    "<foreignObject><iframe src=\"https://evil.example\"></iframe></foreignObject></svg>",
    "<math><annotation-xml encoding=\"text/html\"><script>alert('math')</script></annotation-xml></math>",
    "<video src=\"https://evil.example/v.mp4\" autoplay></video>",
    "<audio src=\"https://evil.example/a.mp3\" autoplay></audio>",
    "<details open ontoggle=\"alert('toggle')\"><summary>s</summary></details>",
    "<p>Closing sentence.</p></article>"
);

/// SEC-002: the attribution section states the text licence each pack's
/// manifest declares, and follows the active pack when it changes.
#[test]
fn the_attribution_states_the_licence_the_active_pack_declares() {
    let lab = Lab::new("pack-licence");
    let src = lab.dir.0.join("src");
    for (id, spdx) in [("licence-30", "CC-BY-SA-3.0"), ("licence-mit", "MIT")] {
        let path = lab.dir.0.join(format!("attacks/{id}.wpack"));
        fs::write(
            &path,
            build_custom_licensed(&src, id, "<p>Body</p>", &[], None, spdx),
        )
        .unwrap();
        lab.library
            .install_pack(path.to_string_lossy().into_owned(), true)
            .unwrap();
    }
    let attribution = |id: &str| {
        lab.library.activate(id.into()).unwrap();
        lab.library
            .open_page("enwiki".into(), 5)
            .unwrap()
            .unwrap()
            .attribution_html
    };
    let older = attribution("licence-30");
    assert!(older.contains("https://creativecommons.org/licenses/by-sa/3.0/"));
    assert!(older.contains(">CC BY-SA 3.0</a>"));
    assert!(!older.contains("by-sa/4.0"));
    let other = attribution("licence-mit");
    assert!(other.contains("the licence the pack names as MIT."));
    assert!(!other.contains("creativecommons.org/licenses"));
    assert!(attribution("licence-30").contains(">CC BY-SA 3.0</a>"));
}

#[test]
fn hostile_article_html_with_valid_hashes_is_sanitised_at_display() {
    let lab = Lab::new("hostile-html");
    let pack = build_custom(
        &lab.dir.0.join("src"),
        "html-attack",
        HOSTILE_ARTICLE,
        &[],
        None,
    );
    let path = lab.dir.0.join("attacks/html.wpack");
    fs::write(&path, &pack).unwrap();
    // Contract: installs (the bytes are exactly what the manifest says) and
    // is cleaned when displayed. Refusing it would also be acceptable, but
    // it must never reach a WebView as-is.
    lab.library
        .install_pack(path.to_string_lossy().into_owned(), true)
        .expect("html is sanitised at display, not refused at install");
    lab.library.activate("html-attack".into()).unwrap();
    let reader = lab
        .library
        .open_reader_page("enwiki".into(), 5, false)
        .unwrap()
        .unwrap();
    let document = reader.document.to_lowercase();
    for needle in [
        "alert(",
        "onclick",
        "onmouseover",
        "onerror",
        "ontoggle",
        "javascript:",
        "vbscript:",
        "data:text/html",
        "file:///",
        "intent:",
        "<iframe",
        "<object",
        "<embed",
        "<form",
        "<input",
        "<base",
        "http-equiv=\"refresh\"",
        "<link",
        "evil.example",
        "tracker.example",
        "<foreignobject",
        "<annotation-xml",
        "<video",
        "<audio",
        "srcset",
    ] {
        assert!(
            !document.contains(needle),
            "reader document still contains {needle:?}:\n{}",
            reader.document
        );
    }
    // Only the app's own script may run in the reader document: its CSP and
    // markup are not the pack's. No pack text becomes a script.
    assert!(!document.contains("script-element") && !document.contains("svg-script"));
    assert!(reader.document.contains("Harmless sentence."));
    assert!(reader.document.contains("Closing sentence."));
    // The hostile pack did not displace the good one.
    lab.library.activate("good-pack".into()).unwrap();
    lab.assert_active_listing(2);
}

impl Lab {
    fn assert_active_listing(&self, count: usize) {
        let packs = self.library.list_packs().unwrap();
        assert_eq!(packs.len(), count);
        assert!(packs.iter().all(|p| !p.damaged));
        assert!(packs
            .iter()
            .any(|p| p.snapshot_id == "good-pack" && p.active));
    }
}

/// Pathological but harmless-looking HTML: the reader must return a
/// document (or a typed error), never overflow its stack or run away. The
/// display sanitiser is superlinear in nesting depth and in character
/// references, so HTML beyond the display limits is shown as a notice
/// without being parsed; the large cases below must come back promptly.
#[test]
fn deeply_nested_and_huge_article_html_does_not_crash_the_reader() {
    let lab = Lab::new("hostile-html-bombs");
    let cases: Vec<(&str, String)> = vec![
        (
            "nested_divs_3000",
            format!("{}x{}", "<div>".repeat(3_000), "</div>".repeat(3_000)),
        ),
        ("unclosed_divs_3000", "<div>".repeat(3_000)),
        (
            "nested_spans_in_links",
            format!("{}x", "<a href=\"./A\"><span>".repeat(3_000)),
        ),
        (
            "many_attributes",
            format!("<p {}>x</p>", "a=\"1\" ".repeat(20_000)),
        ),
        ("million_open_brackets", "<".repeat(1_000_000)),
        ("char_reference_run", "&amp;".repeat(5_000)),
        (
            "nested_divs_200000",
            format!("{}x{}", "<div>".repeat(200_000), "</div>".repeat(200_000)),
        ),
        ("char_reference_flood", "&amp;".repeat(1_500_000)),
        ("huge_text_node", "A".repeat(8 * 1024 * 1024)),
        (
            "deep_inline_svg",
            format!("<svg>{}</svg>", "<g>".repeat(3_000)),
        ),
    ];
    for (n, (name, html)) in cases.iter().enumerate() {
        let id = format!("html-bomb-{n}");
        let pack = build_custom(&lab.dir.0.join("src"), &id, html, &[], None);
        let path = lab.dir.0.join(format!("attacks/{id}.wpack"));
        fs::write(&path, &pack).unwrap();
        // The pack may be refused or installed, but must not take the
        // process down; if installed, so must reading it.
        let installed = lab
            .library
            .install_pack(path.to_string_lossy().into_owned(), true);
        if installed.is_ok() {
            lab.library.activate(id.clone()).unwrap();
            let page = lab
                .library
                .open_reader_page("enwiki".into(), 5, false)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            if let Some(page) = page {
                let lower = page.document.to_lowercase();
                assert!(!lower.contains("<script"), "{name}");
                if name.ends_with("_200000") || *name == "char_reference_flood" {
                    assert!(
                        page.document
                            .contains(wiki_source::reader_document::TOO_COMPLEX_NOTE),
                        "{name}"
                    );
                }
            }
            lab.library.activate("good-pack".into()).unwrap();
            lab.library.remove_pack(id).unwrap();
        }
    }
    lab.assert_good_pack_usable();
}

#[test]
fn hostile_article_title_and_links_cannot_navigate_outside_the_pack() {
    let lab = Lab::new("hostile-titles");
    let library = &lab.library;
    // Navigation titles are validated before any lookup.
    for title in [
        "",
        "../../etc/passwd",
        "a\0b",
        "javascript:alert(1)",
        &"t".repeat(10_000),
    ] {
        let result = library.open_reader_title("enwiki".into(), title.into(), false);
        assert!(
            matches!(result, Ok(None) | Err(LibraryError::InvalidInput)),
            "{title:?}: {result:?}"
        );
    }
    for project in ["../enwiki", "enwiki\0", "", "EN"] {
        let result = library.open_page(project.into(), 1);
        assert!(
            matches!(result, Ok(None) | Err(LibraryError::InvalidInput)),
            "{project:?}: {result:?}"
        );
    }
    // Image reads by hostile "digests" never touch the filesystem outside
    // the snapshot.
    for digest in [
        "../manifest.json",
        "../../../../etc/passwd",
        "/etc/passwd",
        "",
        "a\0b",
    ] {
        assert!(!matches!(library.read_image(digest.into()), Ok(Some(_))));
    }
    lab.assert_good_pack_usable();
}

// ---------------------------------------------------------------------------
// 7. Signed content is not trusted blindly.

const SECRET: [u8; 32] = [0x42; 32];

fn test_keyring() -> [TrustedKey; 1] {
    [TrustedKey {
        publisher: "test-publisher",
        public_key: signature::public_key(&SECRET),
        valid_from: "20260101",
        valid_until: None,
        revoked: false,
    }]
}

fn official(parts: &Parts) -> Vec<u8> {
    parts.pack(
        |_| {},
        |m| {
            m.origin = Origin::Official {
                publisher: "test-publisher".into(),
            }
        },
    )
}

fn signed(pack: &[u8]) -> (Manifest, signature::PackSignature) {
    let manifest = verify_container(&mut Cursor::new(pack)).unwrap();
    let signature = signature::sign(&manifest, &SECRET).unwrap();
    (manifest, signature)
}

fn install_signed(
    root: &Path,
    pack: &[u8],
    signature: &signature::PackSignature,
    keyring: &[TrustedKey],
    max: u64,
) -> Result<wiki_store::pack_install::InstalledPack, PackInstallError> {
    install_official_pack(
        root,
        &mut Cursor::new(pack),
        max,
        PublisherProof { signature, keyring },
        |_| true,
    )
}

fn scratch_root(label: &str) -> (support::TestDir, PathBuf) {
    let dir = test_dir(label);
    let root = fs::canonicalize(&dir.0).unwrap();
    (dir, root)
}

#[test]
fn a_validly_signed_pack_installs_as_the_control_for_the_signed_attacks() {
    let lab = Lab::new("hostile-signed-control");
    let pack = official(&lab.parts());
    let (_, signature) = signed(&pack);
    let (_dir, root) = scratch_root("hostile-signed-control-root");
    let installed = install_signed(&root, &pack, &signature, &test_keyring(), 1 << 30)
        .expect("a correctly signed, well-formed pack installs");
    assert_eq!(
        installed.trust,
        wiki_pack_format::trust::TrustStatus::VerifiedPublisher
    );
    // And through the stream installer.
    let (_dir2, root2) = scratch_root("hostile-signed-control-root2");
    install_official_pack_stream(
        &root2,
        &mut Cursor::new(pack.clone()),
        1 << 30,
        PublisherProof {
            signature: &signature,
            keyring: &test_keyring(),
        },
        |_| true,
    )
    .unwrap();
    let _ = PackTrust::VerifiedPublisher;
}

#[test]
fn a_signature_does_not_excuse_tampered_object_bytes() {
    let lab = Lab::new("hostile-signed-tampered");
    let parts = lab.parts();
    let pack = official(&parts);
    let (_, signature) = signed(&pack);
    let (_dir, root) = scratch_root("hostile-signed-tampered-root");
    // Flip one byte of the last object; the signed manifest still names the
    // original hash.
    let mut tampered = pack.clone();
    *tampered.last_mut().unwrap() ^= 1;
    let error = expect_refusal(
        "signed_tampered_object",
        install_signed(&root, &tampered, &signature, &test_keyring(), 1 << 30),
    );
    assert!(
        matches!(
            error,
            PackInstallError::Container(ContainerError::ObjectDigest(_))
        ),
        "{error:?}"
    );
    // A trailing byte after a signed pack.
    let mut trailing = pack.clone();
    trailing.push(0);
    let error = expect_refusal(
        "signed_trailing",
        install_signed(&root, &trailing, &signature, &test_keyring(), 1 << 30),
    );
    assert!(
        matches!(
            error,
            PackInstallError::Container(ContainerError::TrailingBytes)
        ),
        "{error:?}"
    );
    assert_root_clean("signed_tampered", &root);
}

#[test]
fn a_signature_does_not_excuse_exceeding_the_size_quota() {
    let lab = Lab::new("hostile-signed-quota");
    let pack = official(&lab.parts());
    let (_, signature) = signed(&pack);
    for (label, max) in [("one_byte_short", pack.len() as u64 - 1), ("tiny", 64)] {
        let (_dir, root) = scratch_root("hostile-signed-quota-root");
        let error = expect_refusal(
            label,
            install_signed(&root, &pack, &signature, &test_keyring(), max),
        );
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::ResourceBudget | ContainerError::Io(_))
            ),
            "{label}: {error:?}"
        );
        assert_root_clean(label, &root);
        let error = expect_refusal(
            label,
            install_official_pack_stream(
                &root,
                &mut Cursor::new(pack.clone()),
                max,
                PublisherProof {
                    signature: &signature,
                    keyring: &test_keyring(),
                },
                |_| true,
            ),
        );
        assert!(
            matches!(
                error,
                PackInstallError::Container(ContainerError::ResourceBudget | ContainerError::Io(_))
            ),
            "{label} stream: {error:?}"
        );
        assert_root_clean(label, &root);
    }
}

#[test]
fn a_signature_does_not_excuse_unsafe_object_names_or_manifest_structure() {
    let lab = Lab::new("hostile-signed-structure");
    let parts = lab.parts();
    // Anything the manifest validator accepts can be signed. What it
    // rejects cannot even be signed; what the installer rejects after
    // validation must still be rejected for a signed pack.
    for (name, path) in [
        ("signed_reserved_manifest_json", "manifest.json"),
        ("signed_sqlite_sidecar", "catalog.sqlite-wal"),
        ("signed_undeclared_search_index", "search.sqlite"),
    ] {
        let pack = parts.pack(
            |objects| objects.push((path.into(), b"signed but reserved".to_vec())),
            |m| {
                m.origin = Origin::Official {
                    publisher: "test-publisher".into(),
                }
            },
        );
        let (_, signature) = signed(&pack);
        let (_dir, root) = scratch_root("hostile-signed-structure-root");
        let error = expect_refusal(
            name,
            install_signed(&root, &pack, &signature, &test_keyring(), 1 << 30),
        );
        assert!(
            matches!(error, PackInstallError::UnsupportedObject(_)),
            "{name}: {error:?}"
        );
        assert_root_clean(name, &root);
    }
    // A traversal path cannot be signed at all: signing validates first.
    let hostile = parts.pack(
        |objects| objects[0].0 = "../escape".into(),
        |m| {
            m.origin = Origin::Official {
                publisher: "test-publisher".into(),
            }
        },
    );
    assert!(verify_container(&mut Cursor::new(&hostile)).is_err());
    // Wrong counts, signed.
    let wrong_counts = parts.pack(
        |_| {},
        |m| {
            m.origin = Origin::Official {
                publisher: "test-publisher".into(),
            };
            m.metadata.counts.articles = 50;
        },
    );
    let (_, signature) = signed(&wrong_counts);
    let (_dir, root) = scratch_root("hostile-signed-counts-root");
    let error = expect_refusal(
        "signed_wrong_counts",
        install_signed(&root, &wrong_counts, &signature, &test_keyring(), 1 << 30),
    );
    assert!(
        matches!(error, PackInstallError::CountMismatch { .. }),
        "{error:?}"
    );
    assert_root_clean("signed_wrong_counts", &root);
}

#[test]
fn a_signature_does_not_excuse_hostile_images_or_svg() {
    let lab = Lab::new("hostile-signed-media");
    let signed_install = |name: &str, pack: Vec<u8>| -> PackInstallError {
        let parts = Parts::of(&pack);
        let pack = official(&parts);
        let (_, signature) = signed(&pack);
        let (_dir, root) = scratch_root("hostile-signed-media-root");
        let error = expect_refusal(
            name,
            install_signed(&root, &pack, &signature, &test_keyring(), 1 << 30),
        );
        assert_root_clean(name, &root);
        error
    };
    let svg = |svg: &str| svg_pack(&lab, "signed-svg", svg.as_bytes());
    let error = signed_install(
        "signed_script_svg",
        svg("<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"),
    );
    assert!(
        matches!(error, PackInstallError::Media(PackMediaError::UnsafeSvg(_))),
        "{error:?}"
    );
    let error = signed_install(
        "signed_external_svg",
        svg("<svg xmlns=\"http://www.w3.org/2000/svg\"><image href=\"https://evil.example/x.png\"/></svg>"),
    );
    assert!(
        matches!(error, PackInstallError::Media(PackMediaError::UnsafeSvg(_))),
        "{error:?}"
    );
    let error = signed_install(
        "signed_html_as_png",
        image_pack(
            &lab,
            "signed-html-png",
            "image/png",
            b"<script>alert(1)</script>",
        ),
    );
    assert!(matches!(error, PackInstallError::Media(_)), "{error:?}");
    let error = signed_install(
        "signed_png_as_svg",
        image_pack(&lab, "signed-png-svg", "image/svg+xml", &RED_PNG),
    );
    assert!(
        matches!(error, PackInstallError::Media(PackMediaError::UnsafeSvg(_))),
        "{error:?}"
    );
}

#[test]
fn signed_article_html_is_still_sanitised_at_display() {
    // Trust in the publisher is about origin, not markup: the reader runs
    // the same sanitiser on every article whatever the pack's trust status.
    let lab = Lab::new("hostile-signed-html");
    let pack = build_custom(
        &lab.dir.0.join("src"),
        "signed-html",
        HOSTILE_ARTICLE,
        &[],
        None,
    );
    let pack = official(&Parts::of(&pack));
    let (_, signature) = signed(&pack);
    let (_dir, root) = scratch_root("hostile-signed-html-root");
    install_signed(&root, &pack, &signature, &test_keyring(), 1 << 30).unwrap();
    // Open the signed snapshot through the library: its test key is not
    // pinned in the app, so it is listed as failed verification; whatever
    // the library will show of it must still be sanitised.
    let library = OfflineLibrary::new(root.to_string_lossy().into_owned()).unwrap();
    let listed = library.list_packs().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].trust, PackTrust::VerificationFailed);
    if library.activate("signed-html".into()).is_ok() {
        let page = library
            .open_reader_page("enwiki".into(), 5, false)
            .unwrap()
            .unwrap();
        let document = page.document.to_lowercase();
        for needle in [
            "alert(",
            "javascript:",
            "onclick",
            "<iframe",
            "evil.example",
        ] {
            assert!(!document.contains(needle), "{needle}");
        }
    }
}

#[test]
fn signatures_that_do_not_match_the_pack_or_the_keyring_are_refused() {
    let lab = Lab::new("hostile-bad-signatures");
    let pack = official(&lab.parts());
    let (manifest, signature) = signed(&pack);
    let keyring = test_keyring();
    let (_dir, root) = scratch_root("hostile-bad-signature-root");
    let refused = |name: &str,
                   signature: &signature::PackSignature,
                   ring: &[TrustedKey],
                   want: SignatureError| {
        let error = expect_refusal(name, install_signed(&root, &pack, signature, ring, 1 << 30));
        assert!(
            matches!(&error, PackInstallError::PublisherVerificationFailed(got) if *got == want),
            "{name}: {error:?}"
        );
        assert_root_clean(name, &root);
    };
    let mut flipped = signature.clone();
    flipped.signature[0] ^= 1;
    refused(
        "signature_bit_flipped",
        &flipped,
        &keyring,
        SignatureError::BadSignature,
    );
    let mut other_transcript = signature.clone();
    other_transcript.transcript_sha256[0] ^= 1;
    refused(
        "signature_for_another_pack",
        &other_transcript,
        &keyring,
        SignatureError::TranscriptMismatch,
    );
    let mut unknown = signature.clone();
    unknown.key_id = [9; 8];
    refused(
        "unknown_key_id",
        &unknown,
        &keyring,
        SignatureError::UnknownKey,
    );
    refused("empty_keyring", &signature, &[], SignatureError::UnknownKey);
    let mut revoked = keyring.clone();
    revoked[0].revoked = true;
    refused(
        "revoked_key",
        &signature,
        &revoked,
        SignatureError::RevokedKey,
    );
    let mut wrong_publisher = keyring.clone();
    wrong_publisher[0].publisher = "another-publisher";
    refused(
        "publisher_mismatch",
        &signature,
        &wrong_publisher,
        SignatureError::PublisherMismatch,
    );
    let mut expired = keyring.clone();
    expired[0].valid_until = Some("20260101");
    refused(
        "snapshot_after_key_expiry",
        &signature,
        &expired,
        SignatureError::OutsideValidity,
    );
    // A signature made by a different secret key under the same key ID.
    let forged = signature::sign(&manifest, &[0x43; 32]).unwrap();
    let mut forged = forged;
    forged.key_id = signature.key_id;
    refused(
        "signature_by_another_secret",
        &forged,
        &keyring,
        SignatureError::BadSignature,
    );
    // A custom pack with a signature attached is not an official pack.
    let custom = lab.good.clone();
    let error = expect_refusal(
        "signature_on_custom_pack",
        install_signed(&root, &custom, &signature, &keyring, 1 << 30),
    );
    assert!(
        matches!(error, PackInstallError::NotOfficialPack),
        "{error:?}"
    );
    // The signature file parser: size and shape.
    for text in [
        String::new(),
        "x".repeat(100_000),
        "foundation-wikipedia/pack-signature/v1\n".into(),
        format!("{}\nextra\n", signature.encode().trim_end()),
        signature.encode().replace('\n', "\r\n"),
    ] {
        assert_eq!(
            signature::PackSignature::parse(text.as_bytes()).err(),
            Some(SignatureError::Malformed),
            "{:?}",
            &text[..text.len().min(40)]
        );
    }
    assert_root_clean("bad_signatures", &root);
}
