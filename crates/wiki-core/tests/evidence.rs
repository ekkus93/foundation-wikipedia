use wiki_core::evidence::{evidence_blocks, evidence_references, evidence_sections};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Reference, Revision, Section, ARTICLE_SCHEMA_VERSION,
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
        namespace: 0,
        aliases: vec![],
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

#[test]
fn reference_handles_preserve_revision_and_source_identity() {
    let mut first = article();
    first.references.push(Reference {
        id: "footnote:é".into(),
        label: "Source note".into(),
        source_url: Some("https://example.org/reference".into()),
    });
    let original = evidence_references(&first).unwrap();
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].reference.label, "Source note");
    assert!(original[0].id.starts_with("wkr:enwiki:99:7:"));
    let mut second = first.clone();
    second.revision.revision_id = 8;
    assert_ne!(original[0].id, evidence_references(&second).unwrap()[0].id);
    second.references[0].id = "footnote-é".into();
    assert_ne!(original[0].id, evidence_references(&second).unwrap()[0].id);
    second.references.push(second.references[0].clone());
    assert!(evidence_references(&second).is_err());
}

#[test]
fn nested_sections_have_revision_scoped_hard_handles() {
    let mut fixture = article();
    fixture.sections[0].subsections.push(Section {
        ordinal: 2,
        heading: "Local structure".into(),
        blocks: vec![],
        subsections: vec![],
    });
    let handles = evidence_sections(&fixture).unwrap();
    assert_eq!(handles.len(), 2);
    assert_eq!(handles[0].id, "wks:enwiki:99:7:3");
    assert_eq!(handles[1].id, "wks:enwiki:99:7:3:2");
    assert_eq!(handles[1].headings, ["Space", "Local structure"]);
    assert_eq!(handles[1].section.heading, "Local structure");

    let mut next_revision = fixture.clone();
    next_revision.revision.revision_id = 8;
    let next = evidence_sections(&next_revision).unwrap();
    assert_ne!(handles[0].id, next[0].id);
    assert_ne!(handles[1].id, next[1].id);
}

#[test]
fn section_ids_reject_invalid_article_structure() {
    let mut fixture = article();
    fixture.sections.push(fixture.sections[0].clone());
    assert!(evidence_sections(&fixture).is_err());
}
