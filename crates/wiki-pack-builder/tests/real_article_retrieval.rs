//! RAG-001 on real Wikipedia text: the exact enwiki revision 1383416967 of
//! "Hypotenuse" (fixture with provenance), read into blocks by the SRC-003
//! reader, answers a small labelled question set with the structural BM25
//! retriever — no vector database. Each question names text that only its
//! answer block contains; unanswerable questions must abstain.

use wiki_model::ArticleKey;
use wiki_search::retrieval::{Retrieval, RetrievalRequest};
use wiki_search::ArticleLexicalIndex;
use wiki_source::online_article::{parse_rendered_revision_with_source, OnlineRevision};
use wiki_source::rendered_article::{assemble_article, extract_rendered_article};
use wiki_source::rendered_media::parse_image_metadata;

const PARSE: &str =
    include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.parse.json");
const IMAGES: &str =
    include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.images.json");
const REVISION: u64 = 1383416967;

fn article() -> wiki_model::Article {
    let pinned = OnlineRevision {
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 251366,
        },
        title: "Hypotenuse".into(),
        namespace: 0,
        revision_id: REVISION,
        timestamp: "2026-11-02T15:48:37Z".into(),
    };
    let (rendered, raw) = parse_rendered_revision_with_source(PARSE, &pinned).unwrap();
    let extracted = extract_rendered_article(&raw).unwrap();
    let files: Vec<String> = extracted
        .images
        .iter()
        .map(|i| i.file_title.clone())
        .collect();
    let media = parse_image_metadata(IMAGES, &files).unwrap();
    assemble_article(&pinned, "en", extracted, rendered.html, media).unwrap()
}

fn ask(index: &ArticleLexicalIndex, question: &str) -> Retrieval {
    index.retrieve(&RetrievalRequest {
        query: question.to_owned(),
        expected_revision: REVISION,
        current_block_id: None,
        selected_text: None,
        limit: 6,
        max_context_tokens: 900,
    })
}

/// (question, text only the answer block contains).
const LABELLED: [(&str, &str); 8] = [
    (
        "Which side of a right triangle is the hypotenuse?",
        "opposite to the right angle",
    ),
    (
        "What Ancient Greek word does hypotenuse come from?",
        "hypoteinousa",
    ),
    (
        "When did the word hypotenuse first appear in English?",
        "1570s",
    ),
    (
        "What does the Pythagorean theorem say about the hypotenuse?",
        "a^2+b^2=c^2",
    ),
    (
        "How does the law of cosines relate to the hypotenuse?",
        "law of cosines",
    ),
    ("What does SOH-CAH-TOA stand for?", "SOH-CAH-TOA"),
    (
        "What does the hypot function calculate in programming languages?",
        "hypot(x, y)",
    ),
    (
        "What does C++17 std::hypot compute for a cuboid?",
        "rectangular cuboid",
    ),
];

const UNANSWERABLE: [&str; 2] = [
    "Who won the 1998 FIFA World Cup?",
    "What is the capital city of France?",
];

#[test]
fn labelled_questions_retrieve_their_answer_block_from_a_real_revision() {
    let article = article();
    let index = ArticleLexicalIndex::build(&article).unwrap();
    let mut top1 = 0;
    let mut top3 = 0;
    let mut report = String::new();
    for (question, answer) in LABELLED {
        let Retrieval::Evidence {
            blocks,
            used_tokens,
            ..
        } = ask(&index, question)
        else {
            panic!("no evidence for answerable question {question:?}");
        };
        assert!(used_tokens <= 900, "token budget respected");
        let rank = blocks
            .iter()
            .position(|block| block.hit.excerpt.contains(answer));
        report.push_str(&format!("{question:?} -> rank {rank:?}\n"));
        if rank == Some(0) {
            top1 += 1;
        }
        if rank.is_some_and(|rank| rank < 3) {
            top3 += 1;
        }
    }
    println!("{report}top-1 {top1}/8, top-3 {top3}/8");
    // Every labelled answer is among the passages sent to the model, and
    // nearly always first.
    assert_eq!(top3, LABELLED.len(), "{report}");
    assert!(top1 >= 7, "{report}");

    // Unanswerable questions get no padding: either no evidence at all, or
    // only passages that really contain one of the question's content words
    // (a reference list mentioning "New York City" shares "city" with the
    // France question; the model and citation validation reject it later).
    assert_eq!(ask(&index, UNANSWERABLE[0]), Retrieval::NoEvidence);
    for question in UNANSWERABLE {
        if let Retrieval::Evidence { blocks, .. } = ask(&index, question) {
            let words: Vec<String> = question
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 4)
                .map(str::to_lowercase)
                .collect();
            for block in blocks {
                let text = block.hit.excerpt.to_lowercase();
                assert!(
                    words.iter().any(|w| text.contains(w.as_str())),
                    "{question}: unrelated passage {:?}",
                    block.hit.block_id
                );
            }
        }
    }
    // A conversation pinned to another revision is refused.
    assert!(matches!(
        index.retrieve(&RetrievalRequest {
            query: "hypotenuse".into(),
            expected_revision: REVISION - 1,
            current_block_id: None,
            selected_text: None,
            limit: 3,
            max_context_tokens: 900,
        }),
        Retrieval::RevisionMismatch { .. }
    ));
}
