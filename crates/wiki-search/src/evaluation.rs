//! Deterministic lexical retrieval evaluation over revision-scoped gold blocks.
//!
//! This is an offline diagnostic harness, not a factual-entailment evaluator:
//! a retrieved passage can be topically relevant without supporting an answer.
//! No model, network service or publisher-authenticated corpus is implied.

use std::collections::BTreeSet;

use serde::Serialize;

use wiki_model::ArticleKey;

use crate::{CorpusLexicalIndex, MAX_LEXICAL_QUERY_BYTES};

const MAX_CASES: usize = 1_000;
const MAX_GOLD_PER_CASE: usize = 128;

/// Exact, revision-bound evidence that a labeled question should retrieve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldEvidence {
    pub article: ArticleKey,
    pub revision_id: u64,
    pub block_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievalCase {
    pub id: String,
    pub query: String,
    /// Restrict retrieval to an edition before ranking. None evaluates all.
    pub project: Option<String>,
    /// Empty gold denotes a labeled no-answer retrieval case.
    pub relevant: Vec<GoldEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluationError {
    EmptySuite,
    TooManyCases,
    InvalidLimit,
    InvalidCaseId,
    DuplicateCaseId(String),
    InvalidQuery(String),
    InvalidProject(String),
    TooManyGold(String),
    DuplicateGold(String),
    UnknownOrStaleGold(String),
    OutOfScopeGold(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CaseScore {
    pub id: String,
    pub relevant_count: usize,
    pub found_count: usize,
    pub retrieved_count: usize,
    pub no_answer_false_positive: bool,
}

/// Integer numerators/denominators keep reports reproducible and avoid
/// rounding ambiguities. Recall = found/relevant; precision = found/retrieved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RetrievalScore {
    pub top_k: usize,
    pub cases: Vec<CaseScore>,
    pub answerable_cases: usize,
    pub no_answer_cases: usize,
    pub relevant_total: usize,
    pub found_total: usize,
    pub retrieved_total: usize,
    pub no_answer_false_positive_cases: usize,
}

/// Score a bounded, explicitly labeled corpus suite at top-k. Gold identities
/// must exist in the exact indexed revision, so stale or fabricated gold cannot
/// silently depress measured recall. Case order does not affect the report.
pub fn evaluate_retrieval(
    index: &CorpusLexicalIndex,
    cases: &[RetrievalCase],
    top_k: usize,
) -> Result<RetrievalScore, EvaluationError> {
    if cases.is_empty() {
        return Err(EvaluationError::EmptySuite);
    }
    if cases.len() > MAX_CASES {
        return Err(EvaluationError::TooManyCases);
    }
    if !(1..=100).contains(&top_k) {
        return Err(EvaluationError::InvalidLimit);
    }

    let mut seen_ids = BTreeSet::new();
    let mut scored = Vec::with_capacity(cases.len());
    let mut answerable_cases = 0usize;
    let mut no_answer_cases = 0usize;
    let mut relevant_total = 0usize;
    let mut found_total = 0usize;
    let mut retrieved_total = 0usize;
    let mut no_answer_false_positive_cases = 0usize;

    for case in cases {
        if case.id.is_empty() || case.id.trim() != case.id || case.id.len() > 255 {
            return Err(EvaluationError::InvalidCaseId);
        }
        if !seen_ids.insert(case.id.as_str()) {
            return Err(EvaluationError::DuplicateCaseId(case.id.clone()));
        }
        if case.query.trim().is_empty() || case.query.len() > MAX_LEXICAL_QUERY_BYTES {
            return Err(EvaluationError::InvalidQuery(case.id.clone()));
        }
        if let Some(project) = &case.project {
            if (ArticleKey {
                project: project.clone(),
                page_id: 1,
            })
            .validate()
            .is_err()
            {
                return Err(EvaluationError::InvalidProject(case.id.clone()));
            }
        }
        if case.relevant.len() > MAX_GOLD_PER_CASE {
            return Err(EvaluationError::TooManyGold(case.id.clone()));
        }
        let mut gold = BTreeSet::new();
        for expected in &case.relevant {
            if case
                .project
                .as_ref()
                .is_some_and(|project| *project != expected.article.project)
            {
                return Err(EvaluationError::OutOfScopeGold(case.id.clone()));
            }
            let key = (
                expected.article.project.clone(),
                expected.article.page_id,
                expected.revision_id,
                expected.block_id.clone(),
            );
            if !gold.insert(key) {
                return Err(EvaluationError::DuplicateGold(case.id.clone()));
            }
            let exists = index
                .articles
                .get(&(expected.article.project.clone(), expected.article.page_id))
                .is_some_and(|article| {
                    article.entries.iter().any(|entry| {
                        entry.revision_id == expected.revision_id
                            && entry.block_id == expected.block_id
                    })
                });
            if !exists {
                return Err(EvaluationError::UnknownOrStaleGold(case.id.clone()));
            }
        }
        let hits = if let Some(project) = &case.project {
            index.search_project(project, &case.query, top_k)
        } else {
            index.search(&case.query, top_k)
        };
        let found = hits
            .iter()
            .filter(|hit| {
                gold.contains(&(
                    hit.article.project.clone(),
                    hit.article.page_id,
                    hit.evidence.revision_id,
                    hit.evidence.block_id.clone(),
                ))
            })
            .count();
        let no_answer = gold.is_empty();
        if no_answer {
            no_answer_cases += 1;
            if !hits.is_empty() {
                no_answer_false_positive_cases += 1;
            }
        } else {
            answerable_cases += 1;
        }
        relevant_total += gold.len();
        found_total += found;
        retrieved_total += hits.len();
        scored.push(CaseScore {
            id: case.id.clone(),
            relevant_count: gold.len(),
            found_count: found,
            retrieved_count: hits.len(),
            no_answer_false_positive: no_answer && !hits.is_empty(),
        });
    }
    scored.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(RetrievalScore {
        top_k,
        cases: scored,
        answerable_cases,
        no_answer_cases,
        relevant_total,
        found_total,
        retrieved_total,
        no_answer_false_positive_cases,
    })
}


/** Integer threshold policy for reproducible retrieval regression gates.
 * Recall uses a per-thousand integer ratio; no-answer false positives
 * are counted per case. This does not grade answer factuality.
 */
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RetrievalThresholds {
    pub min_recall_per_mille: u16,
    pub max_no_answer_false_positive_cases: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportError {
    InvalidSuiteId,
    InvalidCommitSha,
    InvalidThreshold,
    Evaluation(EvaluationError),
    Serialization,
}

#[derive(Serialize)]
struct VersionedRetrievalReport<'a> {
    schema_version: u32,
    suite_id: &'a str,
    /// Caller-asserted Git commit identity; this API cannot verify repository state.
    claimed_source_commit_sha: String,
    thresholds: &'a RetrievalThresholds,
    passed: bool,
    retrieval: RetrievalScore,
}

/// Produce a stable, versioned JSON score artifact and an explicit regression
/// decision. The caller is responsible for recording the actual tested commit
/// and authenticating the evaluation corpus. This is retrieval-only evaluation.
pub fn evaluate_retrieval_report_json(
    index: &CorpusLexicalIndex,
    cases: &[RetrievalCase],
    top_k: usize,
    suite_id: &str,
    source_commit_sha: &str,
    thresholds: &RetrievalThresholds,
) -> Result<String, ReportError> {
    if suite_id.is_empty()
        || suite_id.len() > 128
        || !suite_id.as_bytes()[0].is_ascii_alphanumeric()
        || !suite_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ReportError::InvalidSuiteId);
    }
    if source_commit_sha.len() != 40
        || !source_commit_sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ReportError::InvalidCommitSha);
    }
    if thresholds.min_recall_per_mille > 1_000 {
        return Err(ReportError::InvalidThreshold);
    }
    let retrieval = evaluate_retrieval(index, cases, top_k).map_err(ReportError::Evaluation)?;
    let recall_passed = if retrieval.relevant_total == 0 {
        thresholds.min_recall_per_mille == 0
    } else {
        (retrieval.found_total as u128) * 1_000
            >= (thresholds.min_recall_per_mille as u128) * (retrieval.relevant_total as u128)
    };
    let report = VersionedRetrievalReport {
        schema_version: 1,
        suite_id,
        claimed_source_commit_sha: source_commit_sha.to_ascii_lowercase(),
        thresholds,
        passed: recall_passed
            && retrieval.no_answer_false_positive_cases
                <= thresholds.max_no_answer_false_positive_cases,
        retrieval,
    };
    let mut json = serde_json::to_string_pretty(&report).map_err(|_| ReportError::Serialization)?;
    json.push('\n');
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::{Article, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};

    fn article(page_id: u64, title: &str, passage: &str) -> Article {
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id,
            },
            revision: Revision {
                revision_id: 12,
                timestamp: "2026-10-24T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: title.into(),
            display_title: title.into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph(passage.into()),
            }],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<p>fixture</p>".into(),
            is_disambiguation: false,
        }
    }

    fn gold(page_id: u64) -> GoldEvidence {
        GoldEvidence {
            article: ArticleKey {
                project: "enwiki".into(),
                page_id,
            },
            revision_id: 12,
            block_id: format!("wkb:enwiki:{page_id}:12:b0"),
        }
    }

    fn suite() -> Vec<RetrievalCase> {
        vec![
            RetrievalCase {
                id: "physics".into(),
                query: "gravity spacetime".into(),
                project: Some("enwiki".into()),
                relevant: vec![gold(9)],
            },
            RetrievalCase {
                id: "math".into(),
                query: "algebra".into(),
                project: Some("enwiki".into()),
                relevant: vec![gold(10)],
            },
            RetrievalCase {
                id: "no-answer".into(),
                query: "dragonfire".into(),
                project: Some("enwiki".into()),
                relevant: vec![],
            },
            RetrievalCase {
                id: "negative-with-match".into(),
                query: "gravity".into(),
                project: Some("enwiki".into()),
                relevant: vec![],
            },
        ]
    }

    #[test]
    fn deterministic_scored_recall_precision_and_no_answer_cases() {
        let index = CorpusLexicalIndex::build(&[
            article(9, "Physics", "Gravity bends spacetime."),
            article(10, "Mathematics", "Algebra describes structures."),
        ])
        .unwrap();
        let cases = suite();
        let score = evaluate_retrieval(&index, &cases, 2).unwrap();
        assert_eq!(score.answerable_cases, 2);
        assert_eq!(score.no_answer_cases, 2);
        assert_eq!(score.relevant_total, 2);
        assert_eq!(score.found_total, 2);
        assert_eq!(score.retrieved_total, 3);
        assert_eq!(score.no_answer_false_positive_cases, 1);
        assert_eq!(score.cases[0].id, "math");
        let mut reversed = cases;
        reversed.reverse();
        assert_eq!(evaluate_retrieval(&index, &reversed, 2), Ok(score));
    }

    #[test]
    fn rejects_stale_fabricated_duplicate_or_out_of_scope_gold() {
        let index = CorpusLexicalIndex::build(&[article(9, "Physics", "Gravity bends spacetime.")])
            .unwrap();
        let mut cases = suite();
        cases.truncate(1);
        cases[0].relevant[0].revision_id = 13;
        assert_eq!(
            evaluate_retrieval(&index, &cases, 2),
            Err(EvaluationError::UnknownOrStaleGold("physics".into()))
        );
        cases[0].relevant[0].revision_id = 12;
        cases[0].relevant.push(gold(9));
        assert_eq!(
            evaluate_retrieval(&index, &cases, 2),
            Err(EvaluationError::DuplicateGold("physics".into()))
        );
        cases[0].relevant.pop();
        cases[0].project = Some("frwiki".into());
        assert_eq!(
            evaluate_retrieval(&index, &cases, 2),
            Err(EvaluationError::OutOfScopeGold("physics".into()))
        );
    }

    #[test]
    fn rejects_invalid_suite_parameters() {
        let index = CorpusLexicalIndex::build(&[]).unwrap();
        assert_eq!(
            evaluate_retrieval(&index, &[], 2),
            Err(EvaluationError::EmptySuite)
        );
        assert_eq!(
            evaluate_retrieval(&index, &suite(), 0),
            Err(EvaluationError::InvalidLimit)
        );
        let valid_index = CorpusLexicalIndex::build(&[
            article(9, "Physics", "Gravity bends spacetime."),
            article(10, "Mathematics", "Algebra describes structures."),
        ])
        .unwrap();
        let mut cases = suite();
        cases.push(cases[0].clone());
        assert_eq!(
            evaluate_retrieval(&valid_index, &cases, 2),
            Err(EvaluationError::DuplicateCaseId("physics".into()))
        );
    }

    #[test]
    fn versioned_report_matches_checked_in_fixture_and_case_order() {
        let index = CorpusLexicalIndex::build(&[
            article(9, "Physics", "Gravity bends spacetime."),
            article(10, "Mathematics", "Algebra describes structures."),
        ])
        .unwrap();
        let cases = suite();
        let policy = RetrievalThresholds {
            min_recall_per_mille: 1_000,
            max_no_answer_false_positive_cases: 0,
        };
        let sha = "A".repeat(40);
        let json =
            evaluate_retrieval_report_json(&index, &cases, 2, "synthetic-v1", &sha, &policy)
                .unwrap();
        assert_eq!(
            json,
            include_str!("../fixtures/retrieval_report_v1.json")
        );
        let mut reversed = cases;
        reversed.reverse();
        assert_eq!(
            evaluate_retrieval_report_json(&index, &reversed, 2, "synthetic-v1", &sha, &policy),
            Ok(json)
        );
    }

    #[test]
    fn report_rejects_unbounded_metadata_and_invalid_policy() {
        let index = CorpusLexicalIndex::build(&[]).unwrap();
        let cases = suite();
        let policy = RetrievalThresholds {
            min_recall_per_mille: 1_001,
            max_no_answer_false_positive_cases: 0,
        };
        assert_eq!(
            evaluate_retrieval_report_json(&index, &cases, 2, "suite", &"a".repeat(40), &policy),
            Err(ReportError::InvalidThreshold)
        );
        let valid = RetrievalThresholds {
            min_recall_per_mille: 0,
            max_no_answer_false_positive_cases: 0,
        };
        assert_eq!(
            evaluate_retrieval_report_json(&index, &cases, 2, "../bad", &"a".repeat(40), &valid),
            Err(ReportError::InvalidSuiteId)
        );
        assert_eq!(
            evaluate_retrieval_report_json(&index, &cases, 2, "suite", "not-a-sha", &valid),
            Err(ReportError::InvalidCommitSha)
        );
    }

    #[test]
    fn empty_gold_requires_zero_minimum_recall() {
        let index = CorpusLexicalIndex::build(&[]).unwrap();
        let cases = vec![RetrievalCase {
            id: "negative".into(),
            query: "unseen".into(),
            project: None,
            relevant: vec![],
        }];
        let policy = RetrievalThresholds {
            min_recall_per_mille: 1,
            max_no_answer_false_positive_cases: 0,
        };
        let report = evaluate_retrieval_report_json(
            &index,
            &cases,
            1,
            "negative-v1",
            &"b".repeat(40),
            &policy,
        )
        .unwrap();
        assert!(report.contains("\"passed\": false"));
    }

}
