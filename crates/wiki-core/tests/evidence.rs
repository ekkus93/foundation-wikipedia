use wiki_core::evidence::evidence_blocks;
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};

fn article() -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 99,
        },
        revision: Revision {
            revision_id: 7,
            timestamp: "2026-10-09T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: "Physics".into(),
        display_title: "Physics".into(),
        language: "en".into(),
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph("Gravity".into()),
        }],
        sections: vec![Section {
            ordinal: 3,
            heading: "Space".into(),
            blocks: vec![Block {
                ordinal: 1,
                content: BlockContent::Paragraph("Spacetime".into()),
            }],
            subsections: vec![],
        }],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "".into(),
        is_disambiguation: false,
    }
}

#[test]
fn nested_blocks_have_exact_page_revision_path() {
    let fixture = article();
    let blocks = evidence_blocks(&fixture).unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].id, "wkb:enwiki:99:7:b0");
    assert_eq!(blocks[1].id, "wkb:enwiki:99:7:3:b1");
    assert_eq!(blocks[1].headings, ["Space"]);
    assert!(matches!(
        blocks[1].block.content,
        BlockContent::Paragraph(_)
    ));
}

#[test]
fn revision_change_changes_every_evidence_handle() {
    let first = article();
    let mut second = article();
    second.revision.revision_id = 8;
    let before = evidence_blocks(&first).unwrap();
    let after = evidence_blocks(&second).unwrap();
    for (old, new) in before.iter().zip(&after) {
        assert_ne!(old.id, new.id);
    }
}
