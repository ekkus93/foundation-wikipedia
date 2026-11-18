//! Shared host-test helpers: temporary directories and small article packs.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use wiki_ffi::android::OfflineLibrary;
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, MediaAsset, PageRecord, Revision,
    ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::PackMetadata;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::pack_install::snapshot_pack_metadata;
use wiki_store::pack_media::{
    encode_media_index, media_object_path, PackImage, MEDIA_DIR, MEDIA_INDEX_OBJECT,
};
use wiki_store::record_codec::append_record_frame;

pub struct TestDir(pub PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn test_dir(label: &str) -> TestDir {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "wiki-ffi-android-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    TestDir(dir)
}

pub fn article(page_id: u64, title: &str, text: &str) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 7000 + page_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: title.into(),
        display_title: title.into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph(text.into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: format!("<article><h1>{title}</h1><p>{text}</p></article>"),
        is_disambiguation: false,
    }
}

/// Build a two-article pack file and return its path.
pub fn build_pack(dir: &Path, pack_id: &str) -> PathBuf {
    build_pack_at(dir, pack_id, 7000)
}

/// Like `build_pack`, with each article's revision ID `base + page_id`
/// (a newer snapshot of the same pages).
pub fn build_pack_at(dir: &Path, pack_id: &str, revision_base: u64) -> PathBuf {
    let tree = dir.join(format!("{pack_id}-tree"));
    fs::create_dir_all(&tree).unwrap();
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for article in [
        {
            // A long page for reader scroll restoration (MOB-004); its text
            // block, used by search and chat, is unchanged.
            let mut moon = article(1, "Moon", "The Moon orbits the Earth.");
            let mut filler = String::new();
            for n in 1..=40 {
                filler.push_str(&format!(
                    "<p>Synthetic paragraph {n} about the Moon, for scrolling.</p>"
                ));
            }
            moon.rendered_html = format!(
                "<article><h1>Moon</h1><p>The Moon orbits the Earth.</p>{filler}</article>"
            );
            moon
        },
        {
            // An in-pack article link and an external link for the reader
            // (MOB-002); the text blocks used by search and chat are unchanged.
            let mut tide = article(2, "Tide", "Tides follow the Moon's gravity.");
            tide.rendered_html = concat!(
                "<article><p>Tides follow the <a href=\"./Moon\">Moon</a>'s gravity.</p>",
                "<p><a href=\"https://en.wikipedia.org/wiki/Tide\">Tide on Wikipedia</a></p>",
                "</article>"
            )
            .into();
            tide
        },
    ] {
        let mut article = article;
        article.revision.revision_id = revision_base + article.key.page_id;
        let offset = shard.len() as u64;
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article))).unwrap();
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    for (offset, bytes) in frames {
        catalog
            .insert_verified(&tree, "articles.shard", offset, bytes)
            .unwrap();
    }
    drop(catalog);
    let metadata = snapshot_pack_metadata(
        &tree,
        PackMetadata::synthetic_fixture().source,
        &"0".repeat(64),
        "CC-BY-SA-4.0",
    )
    .unwrap();
    let manifest =
        custom_manifest_from_tree(&tree, pack_id, "enwiki", "20261101", "moon", metadata).unwrap();
    let path = dir.join(format!("{pack_id}.wpack"));
    let mut file = fs::File::create(&path).unwrap();
    write_custom_tree(&tree, &manifest, &mut file).unwrap();
    path
}

pub fn library(dir: &Path) -> std::sync::Arc<OfflineLibrary> {
    OfflineLibrary::new(dir.join("library").to_string_lossy().into_owned()).unwrap()
}

/// A valid 32×32 solid red PNG (212, 0, 0): easy to find in a screenshot.
pub const RED_PNG: [u8; 97] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfc, 0x18, 0xed,
    0xa3, 0x00, 0x00, 0x00, 0x28, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xcd, 0x41, 0x09, 0x00,
    0x00, 0x08, 0x00, 0xb1, 0xeb, 0x9f, 0xcf, 0x40, 0xa6, 0xf0, 0x21, 0x0c, 0xf6, 0x5f, 0x53, 0xa7,
    0x12, 0x08, 0x04, 0x02, 0x81, 0x40, 0x20, 0x10, 0x08, 0x04, 0x5f, 0x82, 0x05, 0x89, 0x97, 0x50,
    0x2e, 0xe5, 0xe7, 0xaa, 0xd0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60,
    0x82,
];

pub fn red_png_sha256() -> String {
    format!("{:x}", Sha256::digest(RED_PNG))
}

/// A 32×32 solid blue square, in the SVG sanitiser's exact form.
pub fn blue_svg() -> Vec<u8> {
    wiki_store::pack_svg::sanitize_svg(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"32\" \
          viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" fill=\"#0000d4\"/></svg>",
    )
    .unwrap()
}

/// A one-article pack ("Right triangle", page 3) with two figures stored as
/// pack images (PACK-003): [`RED_PNG`] and [`blue_svg`], a vector.
/// Deterministic.
pub fn build_image_pack(dir: &Path, pack_id: &str) -> PathBuf {
    build_image_pack_with_svg(dir, pack_id, &blue_svg())
}

/// [`build_image_pack`] with `svg` as the stored bytes of the SVG figure.
pub fn build_image_pack_with_svg(dir: &Path, pack_id: &str, svg: &[u8]) -> PathBuf {
    let digest = red_png_sha256();
    let svg_digest = format!("{:x}", Sha256::digest(svg));
    let mut page = article(3, "Right triangle", "A triangle with one right angle.");
    page.revision.revision_id = 7003;
    page.media = vec![
        MediaAsset {
            source_url: "https://upload.wikimedia.org/wikipedia/commons/8/8a/Red_square.png".into(),
            mime_type: "image/png".into(),
            sha256: Some(digest.clone()),
            license: "CC0".into(),
            creator: "Synthetic test image".into(),
            attribution: "Synthetic test image, CC0".into(),
            is_av_preview: false,
            derivation: None,
        },
        MediaAsset {
            source_url: "https://upload.wikimedia.org/wikipedia/commons/b/b1/Blue_square.svg"
                .into(),
            mime_type: "image/svg+xml".into(),
            sha256: Some(svg_digest.clone()),
            license: "CC0".into(),
            creator: "Synthetic test image".into(),
            attribution: "Synthetic test image, CC0".into(),
            is_av_preview: false,
            derivation: Some(wiki_model::MediaDerivation::SanitizedSvg),
        },
    ];
    page.rendered_html = concat!(
        "<article><p>A triangle with one right angle.</p>",
        "<figure><a href=\"./File:Red_square.png\" class=\"mw-file-description\">",
        "<img src=\"//upload.wikimedia.org/wikipedia/commons/thumb/8/8a/Red_square.png/220px-Red_square.png\" ",
        "alt=\"A red square\" width=\"220\" height=\"220\"></a>",
        "<figcaption>A red square</figcaption></figure>",
        "<figure><a href=\"./File:Blue_square.svg\" class=\"mw-file-description\">",
        "<img src=\"//upload.wikimedia.org/wikipedia/commons/thumb/b/b1/Blue_square.svg/220px-Blue_square.svg.png\" ",
        "alt=\"A blue square\" width=\"220\" height=\"220\"></a>",
        "<figcaption>A blue square (vector)</figcaption></figure></article>"
    )
    .into();

    let tree = dir.join(format!("{pack_id}-tree"));
    fs::create_dir_all(tree.join(MEDIA_DIR)).unwrap();
    let mut shard = Vec::new();
    append_record_frame(&mut shard, &PageRecord::Article(Box::new(page))).unwrap();
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    catalog
        .insert_verified(&tree, "articles.shard", 0, shard.len() as u64)
        .unwrap();
    drop(catalog);
    fs::write(tree.join(media_object_path(&digest)), RED_PNG).unwrap();
    fs::write(tree.join(media_object_path(&svg_digest)), svg).unwrap();
    fs::write(
        tree.join(MEDIA_INDEX_OBJECT),
        encode_media_index(&[
            PackImage {
                sha256: digest,
                mime: "image/png".into(),
                bytes: RED_PNG.len() as u64,
            },
            PackImage {
                sha256: svg_digest,
                mime: "image/svg+xml".into(),
                bytes: svg.len() as u64,
            },
        ])
        .unwrap(),
    )
    .unwrap();
    let metadata = snapshot_pack_metadata(
        &tree,
        PackMetadata::synthetic_fixture().source,
        &"0".repeat(64),
        "CC-BY-SA-4.0",
    )
    .unwrap();
    let manifest =
        custom_manifest_from_tree(&tree, pack_id, "enwiki", "20261101", "images", metadata)
            .unwrap();
    let path = dir.join(format!("{pack_id}.wpack"));
    write_custom_tree(&tree, &manifest, &mut fs::File::create(&path).unwrap()).unwrap();
    path
}
