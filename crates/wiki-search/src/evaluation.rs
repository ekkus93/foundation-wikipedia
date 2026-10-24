//! Deterministic lexical retrieval evaluation over revision-scoped gold blocks.
//!
//! This is an offline diagnostic harness, not a factual-entailment evaluator:
//! a retrieved passage can be topically relevant without supporting an answer.
//! No model, network service or publisher-authenticated corpus is implied.

use std::collections::BTreeSet;

use wiki_model::ArticleKey;

use crate::{CorpusLexicalIndex, MAX_LEXICAL_QUERY_BYTES};

const MAX_CASES: usize = 1_000;
const MAX_GOLD_PER_CASE: usize = 128;

/// Exact, revision-bound evidence that a labeled question should retrieve.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseScore {
    pub id: String,
    pub relevant_count: usize,
    pub found_count: usize,
    pub retrieved_count: usize,
    pub no_answer_false_positive: bool,
}

/// Integer numerators/denominators keep reports reproducible and avoid
/// rounding ambiguities. Recall = found/relevant; precision = found/retrieved.
#[derive(Clone, Debug, PartialEq, Eq)]
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
            block_id: format!("wkb:enwiki:{page_id}:12:0:b0"),
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
        let index = CorpusLexicalIndex::build(&[
            article(9, "Physics", "Gravity bends spacetime."),
        ])
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
        let mut cases = suite();
        cases.push(cases[0].clone());
        assert_eq!(
            evaluate_retrieval(&index, &cases, 2),
            Err(EvaluationError::DuplicateCaseId("physics".into()))
        );
    }
}
