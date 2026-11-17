//! Shared host-test helpers: temporary directories and small article packs.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use wiki_ffi::android::OfflineLibrary;
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::PackMetadata;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::pack_install::snapshot_pack_metadata;
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
