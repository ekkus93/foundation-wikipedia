//! Deterministic, synthetic retrieval evaluation. This is a regression
//! smoke test, not evidence of answer factuality or real Wikipedia fidelity.
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_search::ArticleLexicalIndex;

fn paragraph(ordinal: u32, text: &str) -> Block {
    Block {
        ordinal,
        content: BlockContent::Paragraph(text.into()),
    }
}

fn section(ordinal: u32, name: &str, blocks: Vec<Block>, subsections: Vec<Section>) -> Section {
    Section {
        ordinal,
        heading: name.into(),
        blocks,
        subsections,
    }
}

fn article() -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 17,
        },
        revision: Revision {
            revision_id: 88,
            timestamp: "2026-10-09T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: "Synthetic science overview".into(),
        display_title: "Synthetic science overview".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![paragraph(0, "Kinematics describes motion.")],
        sections: vec![
            section(
                1,
                "Gravity",
                vec![
                    paragraph(0, "Gravity curves spacetime."),
                    paragraph(1, "Gravity acceleration near Earth is about 9.8."),
                ],
                vec![],
            ),
            section(
                2,
                "Heat",
                vec![paragraph(
                    0,
                    "Heat transfers thermal energy between bodies.",
                )],
                vec![],
            ),
            section(
                3,
                "Optics",
                vec![paragraph(0, "Light refraction bends at an interface.")],
                vec![section(
                    1,
                    "Reflection",
                    vec![paragraph(
                        0,
                        "Reflection occurs when light returns from a surface.",
                    )],
                    vec![],
                )],
            ),
            section(
                4,
                "Multilingual concepts",
                vec![paragraph(0, "Gravité is a French word for gravity.")],
                vec![],
            ),
        ],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>synthetic</article>".into(),
        is_disambiguation: false,
    }
}

fn block_id(section: &str) -> String {
    format!("wkb:enwiki:17:88:{section}")
}

#[test]
fn labeled_retrieval_has_full_expected_recall_at_three() {
    let cases = [
        ("gravity curves spacetime", block_id("1:b0")),
        ("gravity acceleration Earth", block_id("1:b1")),
        ("heat thermal energy", block_id("2:b0")),
        ("light refraction interface", block_id("3:b0")),
        ("reflection light surface", block_id("3:1:b0")),
        ("Gravité French word", block_id("4:b0")),
    ];
    let index = ArticleLexicalIndex::build(&article()).unwrap();
    let mut found = 0;
    for (query, expected) in cases {
        let hits = index.search(query, 3);
        assert!(!hits.is_empty(), "expected lexical hit: {query}");
        assert!(
            hits.iter().any(|hit| hit.block_id == expected),
            "relevant block not retrieved for {query}: {hits:?}"
        );
        found += 1;
    }
    assert_eq!(found, cases.len());
}

#[test]
fn multi_section_question_returns_both_canonical_sources() {
    let index = ArticleLexicalIndex::build(&article()).unwrap();
    let hits = index.search("gravity heat", 5);
    assert!(hits.iter().any(|hit| hit.block_id == block_id("1:b0")));
    assert!(hits.iter().any(|hit| hit.block_id == block_id("2:b0")));
}

#[test]
fn unrelated_and_zero_budget_questions_fail_conservatively() {
    let index = ArticleLexicalIndex::build(&article()).unwrap();
    assert!(index.search("quantumflibber nonexistentterm", 3).is_empty());
    assert!(index.search_with_word_budget("gravity", 3, 0).is_empty());
    assert!(index.search("gravity", 0).is_empty());
}

#[test]
fn revision_isolation_applies_to_the_entire_eval_set() {
    let old = article();
    let old_index = ArticleLexicalIndex::build(&old).unwrap();
    let mut new = old.clone();
    new.revision.revision_id += 1;
    let new_index = ArticleLexicalIndex::build(&new).unwrap();
    for hit in old_index.search("gravity heat refraction", 5) {
        assert!(!new_index.contains_evidence(&hit));
    }
}
