//! CI-only synthetic retrieval regression gate.
//!
//! This exercises the versioned suite runner end to end and persists a report.
//! It does NOT measure relevance against an authenticated Wikimedia corpus.

use std::{env, fs, process};

use wiki_model::{Article, ArticleKey, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};
use wiki_search::evaluation::RetrievalThresholds;
use wiki_search::evaluation_suite::run_retrieval_suite_json;
use wiki_search::CorpusLexicalIndex;

fn run() -> Result<(), String> {
    let sha = env::var("GITHUB_SHA")
        .map_err(|_| "GITHUB_SHA must identify the exact checked-out commit".to_owned())?;
    let article = Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 9,
        },
        revision: Revision {
            revision_id: 12,
            timestamp: "2026-10-24T00:00:00Z".into(),
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
            content: BlockContent::Paragraph("Gravity bends spacetime.".into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<p>Gravity bends spacetime.</p>".into(),
        is_disambiguation: false,
    };
    let index = CorpusLexicalIndex::build(&[article])
        .map_err(|error| format!("invalid synthetic corpus: {error:?}"))?;
    let thresholds = RetrievalThresholds {
        min_recall_per_mille: 1_000,
        max_no_answer_false_positive_cases: 0,
    };
    let result = run_retrieval_suite_json(
        &index,
        include_str!("../fixtures/retrieval_suite_v1.json"),
        2,
        &sha,
        &thresholds,
    )
    .map_err(|error| format!("synthetic retrieval evaluation error: {error:?}"))?;
    // Persist even a failed regression result so CI can retain diagnostics.
    fs::write("retrieval-synthetic-report.json", result.report_json)
        .map_err(|error| format!("cannot persist retrieval report: {error}"))?;
    if !result.passed {
        return Err("synthetic retrieval regression gate failed; inspect the report".into());
    }
    println!("Synthetic retrieval gate passed at {sha}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
