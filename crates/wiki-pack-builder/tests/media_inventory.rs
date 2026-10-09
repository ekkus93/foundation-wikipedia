use wiki_model::{
    Article, ArticleKey, Block, BlockContent, MediaAsset, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_builder::media_inventory::{collect_structured_media, InventoryError};

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
