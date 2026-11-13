//! RAG-002: saved citations keep their exact revision and evidence, and after
//! an article update they are marked stale instead of pointing at other text.

use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_search::saved_citation::{
    resolve_saved_citation, CitationStatus, ResolveError, SavedCitation, SavedCitationError,
};
use wiki_search::ArticleLexicalIndex;

/// A label, an edit to a valid record, and the error it must produce.
type TamperCase = (&'static str, fn(&mut SavedCitation), SavedCitationError);

fn paragraph(ordinal: u32, text: &str) -> Block {
    Block {
        ordinal,
        content: BlockContent::Paragraph(text.into()),
    }
}

fn article(revision_id: u64, sections: Vec<(&str, Vec<&str>)>) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 42,
        },
        revision: Revision {
            revision_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "d".repeat(64),
        },
        title: "Comet".into(),
        display_title: "Comet".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![paragraph(0, "A comet is an icy body that releases gas.")],
        sections: sections
            .into_iter()
            .enumerate()
            .map(|(i, (heading, blocks))| Section {
                ordinal: i as u32 + 1,
                heading: heading.into(),
                blocks: blocks
                    .into_iter()
                    .enumerate()
                    .map(|(j, text)| paragraph(j as u32, text))
                    .collect(),
                subsections: vec![],
            })
            .collect(),
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>Comet</article>".into(),
        is_disambiguation: false,
    }
}

fn original() -> Article {
    article(
        100,
        vec![
            (
                "Orbit",
                vec!["Comets follow elliptical orbits around the Sun."],
            ),
            ("Tail", vec!["The tail always points away from the Sun."]),
        ],
    )
}

/// Save the citation for the block whose text starts with `prefix`.
fn saved(article: &Article, prefix: &str) -> SavedCitation {
    let hit = ArticleLexicalIndex::build(article)
        .unwrap()
        .search(prefix, 20)
        .into_iter()
        .find(|hit| hit.excerpt.starts_with(prefix))
        .unwrap();
    SavedCitation::from_evidence(&article.key, &hit, Some("science-2026-11")).unwrap()
}

#[test]
fn same_revision_resolves_to_the_exact_jump_target() {
    let article = original();
    let citation = saved(&article, "The tail always");
    match resolve_saved_citation(&article, &citation).unwrap() {
        CitationStatus::Current(hit) => {
            assert_eq!(hit.block_id, citation.block_id);
            assert_eq!(hit.revision_id, 100);
            assert_eq!(hit.excerpt, "The tail always points away from the Sun.");
        }
        other => panic!("expected current, got {other:?}"),
    }
    assert_eq!(citation.snapshot_id.as_deref(), Some("science-2026-11"));
}

#[test]
fn unchanged_text_after_update_is_stale_with_an_identical_relocation() {
    let citation = saved(&original(), "The tail always");
    // The update inserts a section before "Tail", renumbering its block ID.
    let updated = article(
        101,
        vec![
            (
                "Orbit",
                vec!["Comets follow elliptical orbits around the Sun."],
            ),
            ("Nucleus", vec!["The nucleus is rock and ice."]),
            ("Tail", vec!["The tail always points away from the Sun."]),
        ],
    );
    match resolve_saved_citation(&updated, &citation).unwrap() {
        CitationStatus::Stale {
            cited_revision: 100,
            current_revision: 101,
            same_text_in_current: Some(hit),
        } => {
            assert_ne!(hit.block_id, citation.block_id);
            assert_eq!(hit.revision_id, 101);
            assert_eq!(hit.excerpt, citation.excerpt);
            assert_eq!(hit.heading_path, citation.heading_path);
        }
        other => panic!("expected stale with relocation, got {other:?}"),
    }
}

#[test]
fn changed_moved_or_duplicated_text_is_stale_without_relocation() {
    let citation = saved(&original(), "The tail always");
    let changed = article(
        102,
        vec![("Tail", vec!["The tail usually points away from the Sun."])],
    );
    let moved = article(
        103,
        vec![(
            "Appearance",
            vec!["The tail always points away from the Sun."],
        )],
    );
    let duplicated = article(
        104,
        vec![(
            "Tail",
            vec![
                "The tail always points away from the Sun.",
                "The tail always points away from the Sun.",
            ],
        )],
    );
    for (label, updated) in [
        ("changed", changed),
        ("moved", moved),
        ("duplicated", duplicated),
    ] {
        match resolve_saved_citation(&updated, &citation).unwrap() {
            CitationStatus::Stale {
                cited_revision: 100,
                same_text_in_current: None,
                ..
            } => {}
            other => panic!("{label}: expected stale without relocation, got {other:?}"),
        }
    }
    // The original evidence is still available for "as cited" display.
    assert_eq!(
        citation.as_cited().excerpt,
        "The tail always points away from the Sun."
    );
}

#[test]
fn same_revision_with_different_text_is_unavailable_not_redirected() {
    let citation = saved(&original(), "The tail always");
    // Same revision ID but different content: corruption or a forged
    // article. The citation must not jump anywhere.
    let corrupted = article(
        100,
        vec![
            (
                "Orbit",
                vec!["Comets follow elliptical orbits around the Sun."],
            ),
            ("Tail", vec!["Unrelated replacement text."]),
        ],
    );
    assert_eq!(
        resolve_saved_citation(&corrupted, &citation).unwrap(),
        CitationStatus::Unavailable
    );
    let mut other_page = original();
    other_page.key.page_id = 43;
    assert_eq!(
        resolve_saved_citation(&other_page, &citation).unwrap(),
        CitationStatus::WrongArticle
    );
}

#[test]
fn stored_records_round_trip_and_tampering_is_rejected() {
    let citation = saved(&original(), "Comets follow");
    let json = serde_json::to_string(&citation).unwrap();
    let loaded: SavedCitation = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, citation);
    assert_eq!(loaded.validate(), Ok(()));

    let mut extra: serde_json::Value = serde_json::from_str(&json).unwrap();
    extra["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SavedCitation>(extra).is_err());

    let cases: Vec<TamperCase> = vec![
        (
            "edited excerpt",
            |c| c.excerpt.push('!'),
            SavedCitationError::ExcerptDigestMismatch,
        ),
        (
            "block from another revision",
            |c| c.revision_id = 99,
            SavedCitationError::InconsistentIdentity,
        ),
        (
            "block from another page",
            |c| c.page_id = 7,
            SavedCitationError::InconsistentIdentity,
        ),
        (
            "future schema",
            |c| c.schema_version = 2,
            SavedCitationError::UnsupportedSchema(2),
        ),
        (
            "unsafe snapshot id",
            |c| c.snapshot_id = Some("../x".into()),
            SavedCitationError::InvalidSnapshotId,
        ),
        (
            "oversized excerpt",
            |c| c.excerpt = "x".repeat(64 * 1024 + 1),
            SavedCitationError::TooLarge,
        ),
    ];
    for (label, edit, expected) in cases {
        let mut tampered = citation.clone();
        edit(&mut tampered);
        assert_eq!(tampered.validate(), Err(expected.clone()), "{label}");
        assert_eq!(
            resolve_saved_citation(&original(), &tampered),
            Err(ResolveError::Saved(expected)),
            "{label}"
        );
    }
}
