//! Versioned, bounded JSON input for offline retrieval benchmark suites.
//!
//! Parsed suites are untrusted labels, not authenticated Wikimedia data.
//! Always evaluate against an exact indexed corpus: evaluate_retrieval checks
//! that every gold block exists at the claimed article revision.

use std::collections::BTreeSet;

use serde::Deserialize;
use wiki_model::ArticleKey;

use crate::evaluation::{GoldEvidence, RetrievalCase};
use crate::MAX_LEXICAL_QUERY_BYTES;

const MAX_SUITE_BYTES: usize = 2 * 1024 * 1024;
const MAX_SUITE_CASES: usize = 1_000;
const MAX_CASE_GOLD: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievalSuite {
    pub suite_id: String,
    pub cases: Vec<RetrievalCase>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuiteError {
    TooLarge,
    InvalidJson,
    UnsupportedSchema(u32),
    InvalidSuiteId,
    EmptyCases,
    TooManyCases,
    InvalidCaseId,
    DuplicateCaseId(String),
    InvalidQuery(String),
    InvalidProject(String),
    TooManyGold(String),
    InvalidGold(String),
    DuplicateGold(String),
    OutOfScopeGold(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuite {
    schema_version: u32,
    suite_id: String,
    cases: Vec<RawCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    id: String,
    query: String,
    project: Option<String>,
    relevant: Vec<RawGold>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGold {
    article: RawArticleKey,
    revision_id: u64,
    block_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArticleKey {
    project: String,
    page_id: u64,
}

fn valid_suite_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Parse a version-1 labeled suite. Unknown fields and duplicate JSON object
/// keys fail closed. Cases are sorted by ID for deterministic replay, but this
/// parser does not claim that labels, revisions or corpus provenance are true.
///
/// The caller must subsequently call `evaluate_retrieval` against the exact
/// indexed corpus to reject missing, forged and stale gold evidence.
pub fn parse_retrieval_suite_json(input: &str) -> Result<RetrievalSuite, SuiteError> {
    if input.len() > MAX_SUITE_BYTES {
        return Err(SuiteError::TooLarge);
    }
    let raw: RawSuite = serde_json::from_str(input).map_err(|_| SuiteError::InvalidJson)?;
    if raw.schema_version != 1 {
        return Err(SuiteError::UnsupportedSchema(raw.schema_version));
    }
    if !valid_suite_id(&raw.suite_id) {
        return Err(SuiteError::InvalidSuiteId);
    }
    if raw.cases.is_empty() {
        return Err(SuiteError::EmptyCases);
    }
    if raw.cases.len() > MAX_SUITE_CASES {
        return Err(SuiteError::TooManyCases);
    }

    let mut seen_ids = BTreeSet::new();
    let mut cases = Vec::with_capacity(raw.cases.len());
    for case in raw.cases {
        if case.id.is_empty()
            || case.id.len() > 255
            || case.id.trim() != case.id
            || case.id.chars().any(char::is_control)
        {
            return Err(SuiteError::InvalidCaseId);
        }
        if !seen_ids.insert(case.id.clone()) {
            return Err(SuiteError::DuplicateCaseId(case.id));
        }
        if case.query.trim().is_empty()
            || case.query.len() > MAX_LEXICAL_QUERY_BYTES
            || case.query.chars().any(char::is_control)
        {
            return Err(SuiteError::InvalidQuery(case.id));
        }
        if let Some(project) = &case.project {
            if (ArticleKey {
                project: project.clone(),
                page_id: 1,
            })
            .validate()
            .is_err()
            {
                return Err(SuiteError::InvalidProject(case.id));
            }
        }
        if case.relevant.len() > MAX_CASE_GOLD {
            return Err(SuiteError::TooManyGold(case.id));
        }
        let mut seen_gold = BTreeSet::new();
        let mut relevant = Vec::with_capacity(case.relevant.len());
        for gold in case.relevant {
            let article = ArticleKey {
                project: gold.article.project,
                page_id: gold.article.page_id,
            };
            if article.validate().is_err()
                || gold.revision_id == 0
                || gold.block_id.is_empty()
                || gold.block_id.len() > 512
                || gold.block_id.chars().any(char::is_control)
            {
                return Err(SuiteError::InvalidGold(case.id));
            }
            if case
                .project
                .as_ref()
                .is_some_and(|project| project != &article.project)
            {
                return Err(SuiteError::OutOfScopeGold(case.id));
            }
            if !seen_gold.insert((
                article.project.clone(),
                article.page_id,
                gold.revision_id,
                gold.block_id.clone(),
            )) {
                return Err(SuiteError::DuplicateGold(case.id));
            }
            relevant.push(GoldEvidence {
                article,
                revision_id: gold.revision_id,
                block_id: gold.block_id,
            });
        }
        cases.push(RetrievalCase {
            id: case.id,
            query: case.query,
            project: case.project,
            relevant,
        });
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(RetrievalSuite {
        suite_id: raw.suite_id,
        cases,
    })
}


/** Run a versioned, caller-labeled suite against an exact indexed corpus and
 * return both the stable JSON report and its regression-gate decision.
 *
 * The commit SHA is caller-asserted, not independently verified. Neither
 * parsing nor scoring authenticates Wikimedia data or grades answer support.
 */
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuiteRun {
    pub passed: bool,
    pub report_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuiteRunError {
    Suite(SuiteError),
    Report(crate::evaluation::ReportError),
    InvalidReport,
}

pub fn run_retrieval_suite_json(
    index: &crate::CorpusLexicalIndex,
    suite_json: &str,
    top_k: usize,
    source_commit_sha: &str,
    thresholds: &crate::evaluation::RetrievalThresholds,
) -> Result<SuiteRun, SuiteRunError> {
    let suite = parse_retrieval_suite_json(suite_json).map_err(SuiteRunError::Suite)?;
    let report_json = crate::evaluation::evaluate_retrieval_report_json(
        index,
        &suite.cases,
        top_k,
        &suite.suite_id,
        source_commit_sha,
        thresholds,
    )
    .map_err(SuiteRunError::Report)?;

    #[derive(Deserialize)]
    struct Gate {
        passed: bool,
    }
    let gate: Gate = serde_json::from_str(&report_json)
        .map_err(|_| SuiteRunError::InvalidReport)?;
    Ok(SuiteRun {
        passed: gate.passed,
        report_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::{evaluate_retrieval, EvaluationError};
    use crate::CorpusLexicalIndex;

    const INPUT: &str = r#"{
      "schema_version": 1,
      "suite_id": "synthetic-physics-v1",
      "cases": [
        {"id":"z-no-answer","query":"dragonfire","project":"enwiki","relevant":[]},
        {"id":"a-physics","query":"gravity","project":"enwiki",
         "relevant":[{"article":{"project":"enwiki","page_id":9},
                      "revision_id":12,"block_id":"wkb:enwiki:9:12:b0"}]}
      ]
    }"#;

    #[test]
    fn parses_versioned_suite_in_canonical_order_without_claiming_gold_exists() {
        let suite = parse_retrieval_suite_json(INPUT).unwrap();
        assert_eq!(suite.suite_id, "synthetic-physics-v1");
        assert_eq!(suite.cases[0].id, "a-physics");
        assert_eq!(suite.cases[1].id, "z-no-answer");
        let empty_index = CorpusLexicalIndex::build(&[]).unwrap();
        assert_eq!(
            evaluate_retrieval(&empty_index, &suite.cases, 2),
            Err(EvaluationError::UnknownOrStaleGold("a-physics".into()))
        );
    }

    #[test]
    fn rejects_unsupported_version_unknown_and_duplicate_json_keys() {
        assert_eq!(
            parse_retrieval_suite_json(
                &INPUT.replace("\"schema_version\": 1", "\"schema_version\": 2")
            ),
            Err(SuiteError::UnsupportedSchema(2))
        );
        assert_eq!(
            parse_retrieval_suite_json(&INPUT.replace(
                "\"schema_version\": 1",
                "\"schema_version\": 1, \"surprise\": true"
            )),
            Err(SuiteError::InvalidJson)
        );
        assert_eq!(
            parse_retrieval_suite_json(&INPUT.replace(
                "\"schema_version\": 1",
                "\"schema_version\": 1, \"schema_version\": 1"
            )),
            Err(SuiteError::InvalidJson)
        );
        assert_eq!(
            parse_retrieval_suite_json(
                &INPUT.replace("\"page_id\":9", "\"page_id\":9,\"page_id\":9")
            ),
            Err(SuiteError::InvalidJson)
        );
    }

    #[test]
    fn rejects_duplicate_cases_and_gold_without_silently_deduplicating() {
        let duplicated = INPUT.replace("\"z-no-answer\"", "\"a-physics\"");
        assert_eq!(
            parse_retrieval_suite_json(&duplicated),
            Err(SuiteError::DuplicateCaseId("a-physics".into()))
        );
        let duplicate_gold = INPUT.replace(
            "\"revision_id\":12,\"block_id\":\"wkb:enwiki:9:12:b0\"}",
            "\"revision_id\":12,\"block_id\":\"wkb:enwiki:9:12:b0\"},             {\"article\":{\"project\":\"enwiki\",\"page_id\":9},              \"revision_id\":12,\"block_id\":\"wkb:enwiki:9:12:b0\"}"
        );
        assert_eq!(
            parse_retrieval_suite_json(&duplicate_gold),
            Err(SuiteError::DuplicateGold("a-physics".into()))
        );
    }

    #[test]
    fn rejects_bad_identity_scope_and_unbounded_input() {
        assert_eq!(
            parse_retrieval_suite_json(&INPUT.replace("\"page_id\":9", "\"page_id\":0")),
            Err(SuiteError::InvalidGold("a-physics".into()))
        );
        assert_eq!(
            parse_retrieval_suite_json(&INPUT.replace("\"revision_id\":12", "\"revision_id\":0")),
            Err(SuiteError::InvalidGold("a-physics".into()))
        );
        assert_eq!(
            parse_retrieval_suite_json(&INPUT.replace(
                "\"project\":\"enwiki\",\"page_id\":9",
                "\"project\":\"frwiki\",\"page_id\":9"
            )),
            Err(SuiteError::OutOfScopeGold("a-physics".into()))
        );
        assert_eq!(
            parse_retrieval_suite_json(
                &INPUT.replace("\"query\":\"gravity\"", "\"query\":\"gravity\\nforged\"")
            ),
            Err(SuiteError::InvalidQuery("a-physics".into()))
        );
        assert_eq!(
            parse_retrieval_suite_json(&"x".repeat(MAX_SUITE_BYTES + 1)),
            Err(SuiteError::TooLarge)
        );
    }

    #[test]
    fn runs_parsed_suite_and_exposes_machine_readable_gate_result() {
        use crate::evaluation::{ReportError, RetrievalThresholds};
        use wiki_model::{Article, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};

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
        let index = CorpusLexicalIndex::build(&[article]).unwrap();
        let thresholds = RetrievalThresholds {
            min_recall_per_mille: 1_000,
            max_no_answer_false_positive_cases: 0,
        };
        let sha = "a".repeat(40);
        let passing = run_retrieval_suite_json(&index, INPUT, 2, &sha, &thresholds).unwrap();
        assert!(passing.passed);
        assert!(passing.report_json.contains("\"suite_id\": \"synthetic-physics-v1\""));
        assert!(passing.report_json.contains("\"passed\": true"));

        // A no-answer case that retrieves a passage must fail the gate.
        let false_positive = INPUT.replace("\"dragonfire\"", "\"gravity\"");
        let failing =
            run_retrieval_suite_json(&index, &false_positive, 2, &sha, &thresholds).unwrap();
        assert!(!failing.passed);
        assert!(failing.report_json.contains("\"passed\": false"));

        let stale = INPUT.replace("\"revision_id\":12", "\"revision_id\":13");
        assert_eq!(
            run_retrieval_suite_json(&index, &stale, 2, &sha, &thresholds),
            Err(SuiteRunError::Report(ReportError::Evaluation(
                EvaluationError::UnknownOrStaleGold("a-physics".into())
            )))
        );
        assert_eq!(
            run_retrieval_suite_json(&index, INPUT, 2, "bad-sha", &thresholds),
            Err(SuiteRunError::Report(ReportError::InvalidCommitSha))
        );
        assert!(matches!(
            run_retrieval_suite_json(&index, "{}", 2, &sha, &thresholds),
            Err(SuiteRunError::Suite(SuiteError::InvalidJson))
        ));
    }

}
