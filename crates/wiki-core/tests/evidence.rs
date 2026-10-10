use wiki_core::evidence::{
    evidence_blocks, evidence_footnotes, evidence_references, evidence_sections,
};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Footnote, Reference, Revision, Section,
    ARTICLE_SCHEMA_VERSION,
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
        rendered_html: "<article><p>Gravity</p></article>".into(),
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
fn same_revision_reorder_preserves_ordinal_identity_not_vector_position() {
    let mut fixture = article();
    fixture.sections.push(Section {
        ordinal: 8,
        heading: "Matter".into(),
        blocks: vec![Block {
            ordinal: 4,
            content: BlockContent::Paragraph("Matter has mass".into()),
        }],
        subsections: vec![],
    });
    let before = evidence_blocks(&fixture).unwrap();
    let spacetime_before = before
        .iter()
        .find(|entry| {
            matches!(
                &entry.block.content,
                BlockContent::Paragraph(text) if text == "Spacetime"
            )
        })
        .unwrap()
        .id
        .clone();
    fixture.sections.swap(0, 1);
    let after = evidence_blocks(&fixture).unwrap();
    let spacetime_after = after
        .iter()
        .find(|entry| {
            matches!(
                &entry.block.content,
                BlockContent::Paragraph(text) if text == "Spacetime"
            )
        })
        .unwrap()
        .id
        .clone();
    assert_eq!(spacetime_before, "wkb:enwiki:99:7:3:b1");
    assert_eq!(spacetime_before, spacetime_after);
}

#[test]
fn deleting_evidence_never_reassigns_its_hard_id_to_surviving_text() {
    let mut fixture = article();
    fixture.lead.push(Block {
        ordinal: 5,
        content: BlockContent::Paragraph("Second lead block".into()),
    });
    let before = evidence_blocks(&fixture).unwrap();
    let deleted = before
        .iter()
        .find(|entry| {
            matches!(
                &entry.block.content,
                BlockContent::Paragraph(text) if text == "Gravity"
            )
        })
        .unwrap()
        .id
        .clone();
    fixture.lead.remove(0);
    let after = evidence_blocks(&fixture).unwrap();
    assert!(!after.iter().any(|entry| entry.id == deleted));
    assert_eq!(after[0].id, "wkb:enwiki:99:7:b5");
    assert!(matches!(
        &after[0].block.content,
        BlockContent::Paragraph(text) if text == "Second lead block"
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
fn footnote_and_reference_handles_remain_distinct_and_revision_scoped() {
    let mut first = article();
    first.references.push(Reference {
        id: "source:é".into(),
        label: "External source".into(),
        source_url: Some("https://example.org/source".into()),
    });
    first.sections[0].blocks.push(Block {
        ordinal: 2,
        content: BlockContent::Footnote(Footnote {
            source_id: "cite-note:é".into(),
            label: "1".into(),
            text: "Article-local explanatory note".into(),
            reference_ids: vec!["source:é".into()],
        }),
    });

    let notes = evidence_footnotes(&first).unwrap();
    let refs = evidence_references(&first).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(refs.len(), 1);
    assert!(notes[0].id.starts_with("wkf:enwiki:99:7:"));
    assert!(refs[0].id.starts_with("wkr:enwiki:99:7:"));
    assert_ne!(notes[0].id, refs[0].id);
    assert_eq!(notes[0].block_id, "wkb:enwiki:99:7:3:b2");
    assert_eq!(notes[0].headings, ["Space"]);
    assert_eq!(notes[0].footnote.reference_ids, ["source:é"]);

    let mut second = first.clone();
    second.revision.revision_id = 8;
    let (next_note_id, next_ref_id) = {
        let next_notes = evidence_footnotes(&second).unwrap();
        let next_refs = evidence_references(&second).unwrap();
        assert_ne!(notes[0].id, next_notes[0].id);
        assert_ne!(refs[0].id, next_refs[0].id);
        (next_notes[0].id.clone(), next_refs[0].id.clone())
    };
    assert!(next_note_id.starts_with("wkf:enwiki:99:8:"));
    assert!(next_ref_id.starts_with("wkr:enwiki:99:8:"));

    if let BlockContent::Footnote(footnote) = &mut second.sections[0].blocks[1].content {
        footnote.source_id = "cite-note-é".into();
    }
    assert_ne!(next_note_id, evidence_footnotes(&second).unwrap()[0].id);
}

#[test]
fn invalid_footnote_reference_fails_closed() {
    let mut fixture = article();
    fixture.lead.push(Block {
        ordinal: 4,
        content: BlockContent::Footnote(Footnote {
            source_id: "note-1".into(),
            label: "1".into(),
            text: "Needs a missing reference".into(),
            reference_ids: vec!["missing".into()],
        }),
    });
    assert!(evidence_footnotes(&fixture).is_err());
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
