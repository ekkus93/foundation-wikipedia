//! RAG-001: current-article retrieval with BM25, structural signals, a
//! context-token budget and conservative unanswerable/revision handling.
//! Synthetic article text; this is ranking-behaviour regression, not a
//! relevance benchmark on real Wikipedia.

use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_search::retrieval::{estimate_tokens, Retrieval, RetrievalRequest, RetrievedBlock};
use wiki_search::{validate_claimed_citations, ArticleLexicalIndex};

const REVISION: u64 = 4242;

fn paragraph(ordinal: u32, text: &str) -> Block {
    Block {
        ordinal,
        content: BlockContent::Paragraph(text.into()),
    }
}

fn section(ordinal: u32, heading: &str, blocks: Vec<Block>) -> Section {
    Section {
        ordinal,
        heading: heading.into(),
        blocks,
        subsections: vec![],
    }
}

fn article() -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 9232,
        },
        revision: Revision {
            revision_id: REVISION,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "c".repeat(64),
        },
        title: "Lattice tower".into(),
        display_title: "Lattice tower".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![paragraph(
            0,
            "The lattice tower is a wrought iron landmark in the city.",
        )],
        sections: vec![
            section(
                1,
                "History",
                vec![
                    paragraph(
                        0,
                        "Construction began in 1887 and the tower was completed in 1889 for the fair.",
                    ),
                    paragraph(1, "The engineering company designed and built the tower."),
                ],
            ),
            section(
                2,
                "Design",
                vec![
                    paragraph(0, "The tower is 330 metres tall and held the height record."),
                    paragraph(1, "The iron structure weighs about 7,300 tonnes."),
                ],
            ),
            section(
                3,
                "Tourism",
                vec![
                    paragraph(0, "The tower receives about seven million visitors a year."),
                    paragraph(1, "Visitors reach the top by lift or by the stairs."),
                ],
            ),
        ],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>Lattice tower</article>".into(),
        is_disambiguation: false,
    }
}

fn request(query: &str) -> RetrievalRequest {
    RetrievalRequest {
        query: query.into(),
        expected_revision: REVISION,
        current_block_id: None,
        selected_text: None,
        limit: 5,
        max_context_tokens: 10_000,
    }
}

fn evidence(retrieval: Retrieval) -> (Vec<RetrievedBlock>, usize, bool) {
    match retrieval {
        Retrieval::Evidence {
            blocks,
            used_tokens,
            budget_limited,
        } => (blocks, used_tokens, budget_limited),
        other => panic!("expected evidence, got {other:?}"),
    }
}

fn index() -> ArticleLexicalIndex {
    ArticleLexicalIndex::build(&article()).unwrap()
}

#[test]
fn bm25_ranks_the_block_that_answers_first() {
    let (blocks, _, _) = evidence(index().retrieve(&request("When was construction completed?")));
    assert!(blocks[0]
        .hit
        .excerpt
        .starts_with("Construction began in 1887"));
    // Scores are strictly ordered or tied by block ID.
    for pair in blocks.windows(2) {
        assert!(pair[0].hit.score >= pair[1].hit.score);
    }
}

#[test]
fn heading_match_is_a_structural_signal() {
    let (blocks, _, _) = evidence(index().retrieve(&request("design")));
    assert_eq!(
        blocks.len(),
        2,
        "both Design blocks, matched by heading only"
    );
    for block in &blocks {
        assert_eq!(block.hit.heading_path, vec!["Design".to_owned()]);
        assert!(block.signals.heading);
    }
}

#[test]
fn selected_text_pins_its_block_first() {
    let mut req = request("how heavy is it");
    req.selected_text = Some("about 7,300 tonnes".into());
    let (blocks, _, _) = evidence(index().retrieve(&req));
    assert!(blocks[0].signals.selection);
    assert_eq!(
        blocks[0].hit.excerpt,
        "The iron structure weighs about 7,300 tonnes."
    );
    assert_eq!(blocks.iter().filter(|b| b.signals.selection).count(), 1);
}

#[test]
fn current_section_boost_ranks_up_matching_blocks_only() {
    let index = index();
    let plain = evidence(index.retrieve(&request("visitors"))).0;
    let tourism_block = plain[0].hit.block_id.clone();
    let mut req = request("visitors");
    req.current_block_id = Some(tourism_block.clone());
    let boosted = evidence(index.retrieve(&req)).0;
    assert_eq!(
        boosted.len(),
        plain.len(),
        "boost never adds non-matching blocks"
    );
    let before = plain
        .iter()
        .find(|b| b.hit.block_id == tourism_block)
        .unwrap();
    let after = boosted
        .iter()
        .find(|b| b.hit.block_id == tourism_block)
        .unwrap();
    assert!(after.signals.current_section && !before.signals.current_section);
    let ratio = after.hit.score as f64 / before.hit.score as f64;
    assert!((ratio - 1.25).abs() < 0.01, "ratio {ratio}");
}

#[test]
fn token_budget_keeps_whole_exact_blocks_only() {
    let index = index();
    let all = evidence(index.retrieve(&request("tower"))).0;
    assert!(all.len() >= 3);
    let budget = all[0].estimated_tokens + all[1].estimated_tokens;
    let mut req = request("tower");
    req.max_context_tokens = budget;
    let (blocks, used, limited) = evidence(index.retrieve(&req));
    assert!(limited);
    assert!(used <= budget);
    assert_eq!(
        used,
        blocks.iter().map(|b| b.estimated_tokens).sum::<usize>()
    );
    for block in &blocks {
        assert_eq!(block.estimated_tokens, estimate_tokens(&block.hit));
        assert!(
            index.contains_evidence(&block.hit),
            "evidence is never truncated"
        );
    }
    // Retrieved blocks are valid supplied evidence for citation checking.
    let supplied: Vec<_> = blocks.iter().map(|b| b.hit.clone()).collect();
    let claimed = vec![supplied[0].block_id.clone()];
    assert_eq!(
        validate_claimed_citations(&index, &supplied, &claimed)
            .unwrap()
            .len(),
        1
    );

    req.max_context_tokens = 1;
    let (blocks, used, limited) = evidence(index.retrieve(&req));
    assert!(blocks.is_empty() && used == 0 && limited);
}

#[test]
fn unanswerable_questions_get_no_evidence() {
    let index = index();
    // "the" is in every block; nothing else matches.
    assert_eq!(
        index.retrieve(&request("Who painted the portrait?")),
        Retrieval::NoEvidence
    );
    assert_eq!(
        index.retrieve(&request("quantum chromodynamics")),
        Retrieval::NoEvidence
    );
    // A discriminating term still retrieves.
    assert!(matches!(
        index.retrieve(&request("Who designed the tower?")),
        Retrieval::Evidence { .. }
    ));
}

#[test]
fn other_revisions_and_invalid_requests_are_refused() {
    let index = index();
    let mut req = request("tower");
    req.expected_revision = REVISION + 1;
    assert_eq!(
        index.retrieve(&req),
        Retrieval::RevisionMismatch {
            expected: REVISION + 1,
            actual: REVISION
        }
    );
    for edit in [
        (|r: &mut RetrievalRequest| r.query = "   ".into()) as fn(&mut RetrievalRequest),
        |r| r.limit = 0,
        |r| r.max_context_tokens = 0,
        |r| r.selected_text = Some("x".repeat(8193)),
        |r| r.query = "q".repeat(8193),
    ] {
        let mut req = request("tower");
        edit(&mut req);
        assert_eq!(index.retrieve(&req), Retrieval::InvalidRequest);
    }
}

#[test]
fn retrieval_is_deterministic() {
    let index = index();
    let mut req = request("the tower iron visitors");
    req.selected_text = Some("seven million".into());
    let first = index.retrieve(&req);
    for _ in 0..5 {
        assert_eq!(index.retrieve(&req), first);
    }
}

/// Small labeled set: each answerable question names the block prefix that
/// answers it; it must be ranked first. No-answer questions must abstain.
#[test]
fn labeled_questions_retrieve_their_answer_block_first() {
    let index = index();
    let answerable = [
        ("When did construction begin?", "Construction began in 1887"),
        (
            "Which company built it?",
            "The engineering company designed",
        ),
        (
            "How tall is the tower in metres?",
            "The tower is 330 metres tall",
        ),
        (
            "What does the iron structure weigh?",
            "The iron structure weighs",
        ),
        (
            "How many visitors a year?",
            "The tower receives about seven million",
        ),
        (
            "Can you take the stairs or a lift to the top?",
            "Visitors reach the top",
        ),
        (
            "What city landmark is it, wrought iron?",
            "The lattice tower is a wrought iron",
        ),
    ];
    for (question, expected) in answerable {
        let (blocks, _, _) = evidence(index.retrieve(&request(question)));
        assert!(
            blocks[0].hit.excerpt.starts_with(expected),
            "{question:?} ranked {:?} first",
            blocks[0].hit.excerpt
        );
    }
    for question in [
        "Who painted the portrait?",
        "What is the boiling point of mercury?",
    ] {
        assert_eq!(
            index.retrieve(&request(question)),
            Retrieval::NoEvidence,
            "{question:?}"
        );
    }
}

/// Asking about the article's pervasive subject is answerable even though
/// the term is in every block (regression: it was once treated like "the").
#[test]
fn the_articles_own_subject_term_is_answerable() {
    let mut subject = article();
    subject.lead = vec![paragraph(0, "The tower stands in the city.")];
    subject.sections = vec![
        section(
            1,
            "History",
            vec![paragraph(0, "The tower opened in 1889.")],
        ),
        section(
            2,
            "Design",
            vec![paragraph(0, "The tower is made of iron.")],
        ),
    ];
    let index = ArticleLexicalIndex::build(&subject).unwrap();
    let (blocks, _, _) = evidence(index.retrieve(&request("tower")));
    assert_eq!(blocks.len(), 3);
    // A specific unmatched content term still makes "the"/"tower" matches
    // insufficient on their own.
    assert_eq!(
        index.retrieve(&request("Who painted the tower portrait?")),
        Retrieval::NoEvidence
    );
}
