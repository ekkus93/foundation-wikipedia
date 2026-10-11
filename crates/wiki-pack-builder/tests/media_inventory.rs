use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use wiki_store::media_objects::MediaObjectStore;
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, MediaAsset, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_builder::media_inventory::{
    collect_structured_media, collect_structured_media_notices, verify_structured_media_objects,
    InventoryError,
};

fn sample() -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 21,
        },
        revision: Revision {
            revision_id: 7,
            timestamp: "2026-10-09T00:00:00Z".into(),
            content_sha256: "f".repeat(64),
        },
        title: "Diagrams".into(),
        display_title: "Diagrams".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Media { media_index: 1 },
        }],
        sections: vec![Section {
            ordinal: 1,
            heading: "Illustrations".into(),
            blocks: vec![Block {
                ordinal: 0,
                content: BlockContent::Media { media_index: 0 },
            }],
            subsections: vec![Section {
                ordinal: 1,
                heading: "More illustrations".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Media { media_index: 1 },
                }],
                subsections: vec![],
            }],
        }],
        references: vec![],
        links: vec![],
        media: vec![0, 1]
            .into_iter()
            .map(|index| MediaAsset {
                source_url: format!("https://upload.wikimedia.org/{index}.png"),
                mime_type: "image/png".into(),
                sha256: Some("a".repeat(64)),
                license: "CC BY-SA 4.0".into(),
                creator: "Example contributor".into(),
                attribution: "Example contributor, CC BY-SA 4.0".into(),
                is_av_preview: false,
            })
            .collect(),
        rendered_html: "<article>figure</article>".into(),
        is_disambiguation: false,
    }
}

#[test]
fn recursive_media_references_are_sorted_and_deduplicated() {
    let result = collect_structured_media(&sample()).unwrap();
    assert_eq!(result.referenced_media, vec![0, 1]);
    assert!(!result.rendered_dependencies_verified);
}

#[test]
fn structured_media_notices_preserve_revision_and_unicode_attribution() {
    let mut article = sample();
    article.media[0].creator = "Gravité — 引力".into();
    article.media[0].attribution = "Gravité — 引力, CC BY-SA 4.0".into();
    article.media[1].is_av_preview = true;
    let notices = collect_structured_media_notices(&article).unwrap();
    assert_eq!(notices.len(), 2);
    assert_eq!(
        notices.iter().map(|n| n.media_index).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(notices[0].project, "enwiki");
    assert_eq!(notices[0].page_id, 21);
    assert_eq!(notices[0].revision_id, 7);
    assert_eq!(notices[0].creator, "Gravité — 引力");
    assert_eq!(notices[0].attribution, "Gravité — 引力, CC BY-SA 4.0");
    assert!(notices[1].is_av_preview);
    assert_eq!(notices[1].license, "CC BY-SA 4.0");
    assert_eq!(collect_structured_media_notices(&article).unwrap(), notices);
}

#[test]
fn structured_notice_collection_fails_when_attribution_is_unverified() {
    let mut article = sample();
    article.media[0].license.clear();
    assert!(matches!(
        collect_structured_media_notices(&article),
        Err(InventoryError::InvalidArticle(_))
    ));
    article = sample();
    article.lead[0].content = BlockContent::HtmlFallback("<img src='x'>".into());
    assert_eq!(
        collect_structured_media_notices(&article),
        Err(InventoryError::UnresolvedHtmlFallback)
    );
}

#[test]
fn invalid_media_reference_fails_model_validation() {
    let mut article = sample();
    article.lead[0].content = BlockContent::Media { media_index: 5 };
    assert!(matches!(
        collect_structured_media(&article),
        Err(InventoryError::InvalidArticle(_))
    ));
}

#[test]
fn opaque_html_fallback_cannot_be_silently_classed_offline_ready() {
    let mut article = sample();
    article.sections[0].blocks[0].content =
        BlockContent::HtmlFallback("<img src='external-image.png'>".into());
    assert_eq!(
        collect_structured_media(&article),
        Err(InventoryError::UnresolvedHtmlFallback)
    );
}

#[test]
fn verified_structured_objects_are_hashed_and_deduplicated() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "wiki-builder-media-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let store = MediaObjectStore::new(&root);
    let digest = store.store_bytes(b"diagram").unwrap();
    let mut article = sample();
    article.media[0].sha256 = Some(digest.clone());
    article.media[1].sha256 = Some(digest.clone());
    let required = verify_structured_media_objects(&article, &store).unwrap();
    assert_eq!(required.len(), 1);
    assert_eq!(required[0].digest, digest);
    assert_eq!(required[0].bytes, 7);
    // This only proves typed media references, not HTML/CSS completeness.
    assert!(!collect_structured_media(&article)
        .unwrap()
        .rendered_dependencies_verified);

    article.media[0].sha256 = None;
    assert_eq!(
        verify_structured_media_objects(&article, &store),
        Err(InventoryError::MissingDigest(0))
    );
    article.media[0].sha256 = Some(digest.clone());
    fs::write(root.join(&digest[..2]).join(&digest), b"tampered").unwrap();
    assert_eq!(
        verify_structured_media_objects(&article, &store),
        Err(InventoryError::UnverifiedObject(0))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unresolved_html_fallback_blocks_structured_object_verification() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "wiki-builder-html-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let store = MediaObjectStore::new(&root);
    let mut article = sample();
    article.lead[0].content =
        BlockContent::HtmlFallback("<img src='external.png'>".into());
    assert_eq!(
        verify_structured_media_objects(&article, &store),
        Err(InventoryError::UnresolvedHtmlFallback)
    );
    fs::remove_dir_all(root).unwrap();
}
