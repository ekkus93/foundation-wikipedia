//! PACK-003 through the exported library API: a pack with an image installs,
//! activates and reopens, the page reports the image's digest, and the image
//! reads back verified. An image changed on disk after install is refused.

mod support;

use std::fs;
use std::path::PathBuf;

use support::{
    blue_svg, build_image_pack, build_image_pack_with_svg, library, red_png_sha256, test_dir,
    RED_PNG,
};
use wiki_store::pack_media::media_object_path;

#[test]
fn a_pack_image_installs_activates_and_reads_back_verified() {
    let dir = test_dir("pack-images");
    let digest = red_png_sha256();
    let pack = build_image_pack(&dir.0, "image-pack");

    let library = library(&dir.0);
    library
        .install_pack(pack.to_string_lossy().into_owned(), true)
        .unwrap();
    // Activation reopens the snapshot from its installed manifest.
    library.activate("image-pack".into()).unwrap();
    let opened = library.open_page("enwiki".into(), 3).unwrap().unwrap();
    assert_eq!(opened.images.get("Red_square.png"), Some(&digest));
    assert!(
        opened.still_images.is_empty(),
        "a PNG is not an animation still"
    );

    let image = library.read_image(digest.clone()).unwrap().unwrap();
    assert_eq!(image.mime, "image/png");
    assert_eq!(image.bytes, RED_PNG);
    // The SVG figure is kept as a vector, in the sanitiser's exact form.
    let svg_digest = opened.images.get("Blue_square.svg").unwrap().clone();
    let svg = library.read_image(svg_digest.clone()).unwrap().unwrap();
    assert_eq!(svg.mime, "image/svg+xml");
    assert_eq!(svg.bytes, blue_svg());
    assert_eq!(library.read_image("ab".repeat(32)).unwrap(), None);
    assert_eq!(library.read_image("../manifest.json".into()).unwrap(), None);

    // The reader document points the image at the app's local URL.
    let reader = library
        .open_reader_page("enwiki".into(), 3, false)
        .unwrap()
        .unwrap();
    assert!(reader.document.contains(&format!(
        "src=\"https://foundation.invalid/media/{digest}\""
    )));
    assert!(reader.document.contains(&format!(
        "src=\"https://foundation.invalid/media/{svg_digest}\""
    )));
    assert!(!reader.document.contains("upload.wikimedia.org"));

    // Bytes changed on disk after install are never served.
    let installed = dir
        .0
        .join("library/snapshots/image-pack")
        .join(media_object_path(&digest));
    let mut swapped = RED_PNG;
    swapped[60] ^= 1;
    fs::write(&installed, swapped).unwrap();
    assert!(library.read_image(digest).is_err());
}

/// A custom pack cannot carry an SVG that is not exactly what the
/// sanitiser writes, however it is labelled.
#[test]
fn a_pack_with_an_unsanitised_svg_is_refused() {
    let dir = test_dir("pack-images-hostile-svg");
    let clean = String::from_utf8(blue_svg()).unwrap();
    for svg in [
        clean.replace("/></svg>", "/><script>alert(1)</script></svg>"),
        clean.replace("<rect", "<rect onload=\"alert(1)\""),
        clean.replace("<rect", "<image href=\"https://evil.example/x.png\"/><rect"),
        // Harmless but not in the exact form: still refused.
        clean.replace("<rect", "<!-- x --><rect"),
        "<html><script>alert(1)</script></html>".to_owned(),
    ] {
        let pack = build_image_pack_with_svg(&dir.0, "hostile-svg", svg.as_bytes());
        let library = library(&dir.0.join(format!("lib-{}", svg.len())));
        assert!(
            matches!(
                library.install_pack(pack.to_string_lossy().into_owned(), true),
                Err(wiki_ffi::android::LibraryError::MediaInvalid)
            ),
            "{svg}"
        );
        fs::remove_file(pack).unwrap();
        fs::remove_dir_all(dir.0.join("hostile-svg-tree")).unwrap();
    }
}

fn android_image_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/wpack/android-images-v2.wpack")
}

/// The Android image test installs this committed pack. It must stay
/// exactly what `build_image_pack` produces; regenerate after an intentional
/// change with `WPACK_WRITE_FIXTURE=1 cargo test -p wiki-ffi --test
/// pack_images committed_android_image_fixture` and update its provenance hash.
#[test]
fn committed_android_image_fixture_matches_the_builder() {
    let first = test_dir("image-fixture-a");
    let second = test_dir("image-fixture-b");
    let bytes = fs::read(build_image_pack(&first.0, "android-images")).unwrap();
    assert_eq!(
        bytes,
        fs::read(build_image_pack(&second.0, "android-images")).unwrap(),
        "the pack builder must be deterministic"
    );
    if std::env::var_os("WPACK_WRITE_FIXTURE").is_some() {
        fs::write(android_image_fixture_path(), &bytes).unwrap();
    }
    assert_eq!(fs::read(android_image_fixture_path()).unwrap(), bytes);
}

/// Opt-in audit of a real built pack (PACK-003 evidence): set
/// `PACK003_PACK=<file.wpack>` and run with `--ignored --nocapture`. Every
/// article's every required image must read back verified, and the reader
/// document must load each one locally and nothing else.
/// `PACK003_SHOW_SIZES` lists every image's size, format and source;
/// `PACK003_DUMP_MAPS=<dir>` writes each drawn map there for review.
/// MOBPACK-003: the document must also keep every formula, table,
/// reference mark and reference entry the stored article has.
#[test]
#[ignore = "needs a built pack; see PACK-003 evidence"]
fn audit_a_real_pack_for_complete_local_images() {
    let Some(pack) = std::env::var_os("PACK003_PACK") else {
        panic!("set PACK003_PACK to a .wpack file");
    };
    let dir = test_dir("pack-audit");
    let library = library(&dir.0);
    let installed = library
        .install_pack(pack.to_string_lossy().into_owned(), true)
        .unwrap();
    library.activate(installed.snapshot_id.clone()).unwrap();
    let snapshot = dir.0.join("library/snapshots").join(&installed.snapshot_id);
    let catalog =
        wiki_store::catalog::SnapshotCatalog::open_read_only(&snapshot.join("catalog.sqlite"))
            .unwrap();
    let mut articles = Vec::new();
    catalog
        .visit_records(&snapshot, |record| {
            if let wiki_model::PageRecord::Article(article) = record {
                articles.push(*article);
            }
            Ok::<(), wiki_store::catalog::CatalogError>(())
        })
        .unwrap();
    let (mut figures, mut bytes) = (0, 0);
    for article in &articles {
        let page = library
            .open_reader_page(article.key.project.clone(), article.key.page_id, false)
            .unwrap()
            .unwrap();
        for asset in &article.media {
            let digest = asset.sha256.clone().expect("every image has bytes");
            let image = library.read_image(digest.clone()).unwrap().unwrap();
            assert_eq!(image.mime, asset.mime_type);
            bytes += image.bytes.len();
            if let Some(dir) = std::env::var_os("PACK003_DUMP_MAPS") {
                if asset.derivation == Some(wiki_model::MediaDerivation::RenderedMap) {
                    fs::write(
                        PathBuf::from(dir).join(format!("{digest}.svg")),
                        &image.bytes,
                    )
                    .unwrap();
                }
            }
            if std::env::var_os("PACK003_SHOW_SIZES").is_some() {
                println!(
                    "    {:>9} {} {:?}{} {}",
                    image.bytes.len(),
                    image.mime,
                    asset.derivation,
                    if asset.is_av_preview {
                        " video still"
                    } else {
                        ""
                    },
                    asset.source_url
                );
            }
            assert!(
                page.document.contains(&format!(
                    "src=\"https://foundation.invalid/media/{digest}\""
                )),
                "{}: {} not shown locally",
                article.title,
                asset.source_url
            );
        }
        let sources = page.document.matches(" src=").count();
        let local = page
            .document
            .matches(" src=\"https://foundation.invalid/media/")
            .count();
        assert_eq!(sources, local, "{}: a non-local source", article.title);
        println!(
            "{}: {} required images, {} shown locally, {} <img> total, {} interactive maps noted",
            article.title,
            article.media.len(),
            local,
            page.document.matches("<img").count(),
            page.document
                .matches("class=\"foundation-map-note\"")
                .count()
        );
        assert!(
            !page.document.contains("maps.wikimedia.org"),
            "{}: a map would load",
            article.title
        );
        if std::env::var_os("PACK003_SHOW_UNUSED").is_some() {
            for tag in page.document.split("<img").skip(1) {
                let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
                if !tag.contains(" src=") {
                    println!("    not loaded: <img{}>", &tag[..tag.len().min(160)]);
                }
            }
        }
        // MOBPACK-003: nothing but images may be dropped on the way to the
        // reader: count math, tables and references in the stored HTML and
        // in the document shown.
        let stored = content_counts(&article.rendered_html);
        let shown = content_counts(&page.document);
        println!(
            "    revision {} stored {stored:?}\n    shown  {shown:?}",
            article.revision.revision_id
        );
        assert_eq!(stored, shown, "{}: content lost on display", article.title);
        figures += article.media.len();
    }
    println!(
        "{} articles, {figures} required images, {bytes} image bytes read back verified",
        articles.len()
    );
}

/// Math, tables, reference marks and reference-list entries in `html`.
#[derive(Debug, PartialEq, Eq)]
struct ContentCounts {
    math: usize,
    tables: usize,
    reference_marks: usize,
    reference_entries: usize,
}

fn content_counts(html: &str) -> ContentCounts {
    ContentCounts {
        math: html.matches("<math").count(),
        tables: html.matches("<table").count(),
        reference_marks: html.matches("id=\"cite_ref-").count(),
        reference_entries: html.matches("id=\"cite_note-").count(),
    }
}
