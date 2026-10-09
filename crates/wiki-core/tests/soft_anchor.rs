use wiki_core::soft_anchor::{AnchorMatch, SoftAnchor};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};

fn article() -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 47,
        },
        revision: Revision {
            revision_id: 1,
            timestamp: "2026-10-09T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: "Gravity".into(),
        display_title: "Gravity".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph("Gravity   attracts matter.".into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: String::new(),
        is_disambiguation: false,
    }
}

#[test]
fn soft_anchor_can_move_after_revision_and_block_reordering() {
    let original = article();
    let hard = original.key.block_id(1, &[], 0);
    let anchor = SoftAnchor::capture(&original, &hard).unwrap().unwrap();
    assert_eq!(
        anchor.relocate(&original),
        Ok(AnchorMatch::Located {
            block_id: hard.clone(),
            relocated: false,
        })
    );
    let mut changed = original.clone();
    changed.revision.revision_id = 2;
    changed.lead[0].ordinal = 9;
    changed.lead[0].content = BlockContent::Paragraph("Gravity attracts matter.".into());
    let next = changed.key.block_id(2, &[], 9);
    assert_eq!(
        anchor.relocate(&changed),
        Ok(AnchorMatch::Located {
            block_id: next.clone(),
            relocated: true,
        })
    );
    assert_ne!(hard, next);
}

#[test]
fn missing_duplicate_and_wrong_article_are_not_silently_redirected() {
    let original = article();
    let anchor = SoftAnchor::capture(&original, &original.key.block_id(1, &[], 0))
        .unwrap()
        .unwrap();
    let mut changed = original.clone();
    changed.revision.revision_id = 2;
    changed.lead[0].content = BlockContent::Paragraph("Different content".into());
    assert_eq!(anchor.relocate(&changed), Ok(AnchorMatch::Missing));

    changed.lead[0].content = BlockContent::Paragraph("Gravity attracts matter.".into());
    changed.lead[0].ordinal = 8;
    changed.lead.push(Block {
        ordinal: 9,
        content: BlockContent::Paragraph("Gravity attracts matter.".into()),
    });
    assert_eq!(anchor.relocate(&changed), Ok(AnchorMatch::Ambiguous));

    changed.sections.push(Section {
        ordinal: 1,
        heading: "Other".into(),
        blocks: vec![],
        subsections: vec![],
    });
    changed.key.page_id = 48;
    assert_eq!(anchor.relocate(&changed), Ok(AnchorMatch::WrongArticle));
}

#[test]
fn non_text_block_cannot_form_soft_anchor() {
    let mut original = article();
    original.lead[0].content = BlockContent::Math {
        source: "F = ma".into(),
        html: String::new(),
    };
    let anchor = SoftAnchor::capture(&original, &original.key.block_id(1, &[], 0));
    assert_eq!(anchor.unwrap(), None);
}
