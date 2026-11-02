//! Deterministic scoring of *human-reviewed* generated-answer claims and citations.
//!
//! Unlike retrieval precision, a relevant passage may not support a factual
//! assertion. A reviewer must judge each claim and each claim/citation pair.
//! This library checks annotation completeness and citation-marker integrity;
//! it does NOT infer entailment or authenticate the reviewer or source data.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wiki_model::{Article, ArticleKey};

use crate::ArticleLexicalIndex;

const MAX_SUITE_BYTES: usize = 2 * 1024 * 1024;
const MAX_CASES: usize = 1_000;
const MAX_CLAIMS: usize = 128;
const MAX_CITATIONS_PER_CLAIM: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Supported,
    Unsupported,
    Unreviewed,
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
    project: String,
    page_id: u64,
    revision_id: u64,
    question: String,
    answer: String,
    claims: Vec<RawClaim>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClaim {
    id: String,
    text: String,
    verdict: ReviewVerdict,
    citations: Vec<RawCitation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCitation {
    block_id: String,
    verdict: ReviewVerdict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnswerSupportError {
    TooLarge,
    InvalidJson,
    UnsupportedSchema(u32),
    InvalidSuiteId,
    InvalidCase(String),
    DuplicateCase(String),
    InvalidClaim(String),
    DuplicateClaim(String),
    InvalidCitation(String),
    DuplicateCitation(String),
    MissingCitationMarker(String),
    UnlabeledCitationMarker(String),
    InconsistentSupport(String),
    InvalidCorpus,
    InvalidCorpusArticle(String),
    DuplicateCorpusArticle(String),
    UnknownOrStaleArticle(String),
    UnknownEvidence(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SupportCounts {
    pub claims: usize,
    pub reviewed_claims: usize,
    pub supported_claims: usize,
    pub unsupported_claims: usize,
    pub unreviewed_claims: usize,
    pub uncited_claims: usize,
    pub citations: usize,
    pub reviewed_citations: usize,
    pub supported_citations: usize,
    pub unsupported_citations: usize,
    pub unreviewed_citations: usize,
}

impl SupportCounts {
    fn add(&mut self, other: &Self) {
        self.claims += other.claims;
        self.reviewed_claims += other.reviewed_claims;
        self.supported_claims += other.supported_claims;
        self.unsupported_claims += other.unsupported_claims;
        self.unreviewed_claims += other.unreviewed_claims;
        self.uncited_claims += other.uncited_claims;
        self.citations += other.citations;
        self.reviewed_citations += other.reviewed_citations;
        self.supported_citations += other.supported_citations;
        self.unsupported_citations += other.unsupported_citations;
        self.unreviewed_citations += other.unreviewed_citations;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CaseSupportScore {
    pub id: String,
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    /// Digest of the exact answer text reviewed, not a trust assertion.
    pub answer_sha256: String,
    pub counts: SupportCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnswerSupportReport {
    pub schema_version: u32,
    pub suite_id: String,
    /// Digest of the exact annotation-file bytes for reproducible comparison.
    pub input_sha256: String,
    pub cases: Vec<CaseSupportScore>,
    pub totals: SupportCounts,
    /// Unreviewed labels are excluded from both denominators. Never treat an
    /// empty reviewed denominator as perfect precision or support.
    pub all_claims_reviewed: bool,
    pub all_citations_reviewed: bool,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_id(id: &str, max: usize) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// Canonical article block IDs include revision-bound colon-separated fields
/// (for example `wkb:enwiki:42:100:b0`). Unlike suite/claim names,
/// citation IDs must retain colons rather than rejecting real evidence IDs.
fn valid_citation_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// Validate the structural identity of a canonical block citation against the
/// reviewed case. This is deliberately NOT a membership or authenticity check:
/// the exact article must still be independently verified by the reviewer.
/// Legacy single-token fixture IDs remain accepted for existing test suites.
fn canonical_decimal_u32(value: &str) -> bool {
    value
        .parse::<u32>()
        .is_ok_and(|parsed| parsed.to_string() == value)
}

fn citation_matches_case(id: &str, project: &str, page_id: u64, revision_id: u64) -> bool {
    if !valid_citation_id(id) {
        return false;
    }
    if !id.contains(':') {
        return true;
    }
    let mut parts = id.split(':');
    if parts.next() != Some("wkb")
        || parts.next() != Some(project)
        || parts.next() != Some(page_id.to_string().as_str())
        || parts.next() != Some(revision_id.to_string().as_str())
    {
        return false;
    }
    let mut tail = parts.peekable();
    while let Some(part) = tail.next() {
        if tail.peek().is_none() {
            return part.strip_prefix('b').is_some_and(canonical_decimal_u32);
        }
        if !canonical_decimal_u32(part) {
            return false;
        }
    }
    false
}

/// Parse literal [[cite:BLOCK_ID]] markers; malformed or unterminated markers
/// cannot be smuggled into an apparently annotated answer.
fn answer_markers(
    answer: &str,
    case_id: &str,
) -> Result<(BTreeSet<String>, String), AnswerSupportError> {
    let mut markers = BTreeSet::new();
    // Claims must be anchored in answer prose, not in citation IDs. Keep a
    // separator where a marker was so words on either side cannot join.
    let mut visible_answer = String::with_capacity(answer.len());
    let mut rest = answer;
    while let Some(start) = rest.find("[[cite:") {
        visible_answer.push_str(&rest[..start]);
        visible_answer.push(' ');
        rest = &rest[start + "[[cite:".len()..];
        let end = rest
            .find("]]")
            .ok_or_else(|| AnswerSupportError::InvalidCitation(case_id.to_owned()))?;
        let id = &rest[..end];
        if !valid_citation_id(id) {
            return Err(AnswerSupportError::InvalidCitation(case_id.to_owned()));
        }
        markers.insert(id.to_owned());
        rest = &rest[end + 2..];
    }
    visible_answer.push_str(rest);
    Ok((markers, visible_answer))
}

/// Grade explicit human annotations for generated answers. The reviewer must
/// identify each literal claim in the answer and label every cited block/claim
/// pair separately. A "supported" claim must have at least one individually
/// supported citation; unreviewed annotations are never counted as successes.
///
/// This validates marker identity within the supplied answer, NOT that block
/// IDs exist in an authenticated article nor that a verdict is factually true.
pub fn evaluate_answer_support_json(
    input: &str,
) -> Result<AnswerSupportReport, AnswerSupportError> {
    if input.len() > MAX_SUITE_BYTES {
        return Err(AnswerSupportError::TooLarge);
    }
    let raw: RawSuite = serde_json::from_str(input).map_err(|_| AnswerSupportError::InvalidJson)?;
    if raw.schema_version != 1 {
        return Err(AnswerSupportError::UnsupportedSchema(raw.schema_version));
    }
    if !valid_id(&raw.suite_id, 128) {
        return Err(AnswerSupportError::InvalidSuiteId);
    }
    if raw.cases.is_empty() || raw.cases.len() > MAX_CASES {
        return Err(AnswerSupportError::InvalidCase("suite case count".into()));
    }

    let mut seen_cases = BTreeSet::new();
    let mut cases = Vec::with_capacity(raw.cases.len());
    let mut totals = SupportCounts::default();
    for case in raw.cases {
        if !valid_id(&case.id, 255)
            || (ArticleKey {
                project: case.project.clone(),
                page_id: case.page_id,
            })
            .validate()
            .is_err()
            || case.revision_id == 0
            || case.revision_id > i64::MAX as u64
            || case.question.trim().is_empty()
            || case.question.len() > 8_192
            || case.answer.is_empty()
            || case.answer.len() > 65_536
            || case.answer.contains('\0')
            || case.claims.is_empty()
            || case.claims.len() > MAX_CLAIMS
        {
            return Err(AnswerSupportError::InvalidCase(case.id));
        }
        if !seen_cases.insert(case.id.clone()) {
            return Err(AnswerSupportError::DuplicateCase(case.id));
        }
        let (markers, visible_answer) = answer_markers(&case.answer, &case.id)?;
        let mut labeled_markers = BTreeSet::new();
        let mut seen_claims = BTreeSet::new();
        // Text-only claim annotations have no occurrence offsets. Counting
        // the same literal span under different IDs inflates support scores.
        let mut seen_claim_texts = BTreeSet::new();
        let mut counts = SupportCounts::default();

        for claim in case.claims {
            if !valid_id(&claim.id, 255)
                || claim.text.trim().is_empty()
                || claim.text.len() > 8_192
                || !visible_answer.contains(&claim.text)
                || claim.citations.len() > MAX_CITATIONS_PER_CLAIM
            {
                return Err(AnswerSupportError::InvalidClaim(case.id));
            }
            if !seen_claims.insert(claim.id.clone()) || !seen_claim_texts.insert(claim.text.clone())
            {
                return Err(AnswerSupportError::DuplicateClaim(case.id));
            }
            counts.claims += 1;
            if claim.citations.is_empty() {
                counts.uncited_claims += 1;
            }
            let mut seen_citations = BTreeSet::new();
            let mut supported_citation = false;
            for citation in claim.citations {
                if !citation_matches_case(
                    &citation.block_id,
                    &case.project,
                    case.page_id,
                    case.revision_id,
                ) {
                    return Err(AnswerSupportError::InvalidCitation(case.id));
                }
                if !seen_citations.insert(citation.block_id.clone()) {
                    return Err(AnswerSupportError::DuplicateCitation(case.id));
                }
                if !markers.contains(&citation.block_id) {
                    return Err(AnswerSupportError::MissingCitationMarker(case.id));
                }
                labeled_markers.insert(citation.block_id);
                counts.citations += 1;
                match citation.verdict {
                    ReviewVerdict::Supported => {
                        supported_citation = true;
                        counts.supported_citations += 1;
                        counts.reviewed_citations += 1;
                    }
                    ReviewVerdict::Unsupported => {
                        counts.unsupported_citations += 1;
                        counts.reviewed_citations += 1;
                    }
                    ReviewVerdict::Unreviewed => counts.unreviewed_citations += 1,
                }
            }
            match claim.verdict {
                ReviewVerdict::Supported => {
                    if !supported_citation {
                        return Err(AnswerSupportError::InconsistentSupport(case.id));
                    }
                    counts.supported_claims += 1;
                    counts.reviewed_claims += 1;
                }
                ReviewVerdict::Unsupported => {
                    counts.unsupported_claims += 1;
                    counts.reviewed_claims += 1;
                }
                ReviewVerdict::Unreviewed => counts.unreviewed_claims += 1,
            }
        }
        if markers != labeled_markers {
            return Err(AnswerSupportError::UnlabeledCitationMarker(case.id));
        }
        totals.add(&counts);
        cases.push(CaseSupportScore {
            id: case.id,
            project: case.project,
            page_id: case.page_id,
            revision_id: case.revision_id,
            answer_sha256: sha256(case.answer.as_bytes()),
            counts,
        });
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(AnswerSupportReport {
        schema_version: 1,
        suite_id: raw.suite_id,
        input_sha256: sha256(input.as_bytes()),
        cases,
        all_claims_reviewed: totals.unreviewed_claims == 0 && totals.reviewed_claims > 0,
        all_citations_reviewed: totals.unreviewed_citations == 0 && totals.reviewed_citations > 0,
        totals,
    })
}

/// Strict benchmark mode: independently verify that every reviewer-labeled
/// citation is an actual retrievable block of the supplied canonical article
/// at the exact project/page/revision. This does not authenticate the corpus
/// or reviewer, nor prove that a block factually entails the answer.
pub fn evaluate_answer_support_with_corpus_json(
    input: &str,
    articles: &[Article],
) -> Result<AnswerSupportReport, AnswerSupportError> {
    // Validate labels first, including all literal markers and structural IDs.
    let report = evaluate_answer_support_json(input)?;
    if articles.is_empty() || articles.len() > MAX_CASES {
        return Err(AnswerSupportError::InvalidCorpus);
    }
    let mut indexes = BTreeMap::new();
    for article in articles {
        let key = (article.key.project.clone(), article.key.page_id);
        let index = ArticleLexicalIndex::build(article).map_err(|_| {
            AnswerSupportError::InvalidCorpusArticle(format!("{}:{}", key.0, key.1))
        })?;
        if indexes.insert(key.clone(), index).is_some() {
            return Err(AnswerSupportError::DuplicateCorpusArticle(format!(
                "{}:{}",
                key.0, key.1
            )));
        }
    }
    // The strict mode never accepts legacy fixture aliases: an ID must be
    // minted from a real canonical block and survive an exact index lookup.
    let raw: RawSuite = serde_json::from_str(input).map_err(|_| AnswerSupportError::InvalidJson)?;
    for case in raw.cases {
        let index = indexes
            .get(&(case.project, case.page_id))
            .filter(|index| index.revision_id == case.revision_id)
            .ok_or_else(|| AnswerSupportError::UnknownOrStaleArticle(case.id.clone()))?;
        for claim in case.claims {
            for citation in claim.citations {
                if !citation.block_id.starts_with("wkb:")
                    || !index
                        .entries
                        .iter()
                        .any(|hit| hit.block_id == citation.block_id)
                {
                    return Err(AnswerSupportError::UnknownEvidence(case.id));
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite() -> String {
        serde_json::json!({
            "schema_version": 1,
            "suite_id": "human_review_v1",
            "cases": [{
                "id": "case_1", "project": "enwiki", "page_id": 42,
                "revision_id": 100, "question": "How many moons?",
                "answer": "Two moons [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].",
                "claims": [
                    { "id": "c1", "text": "Two moons", "verdict": "supported",
                      "citations": [{"block_id": "lead_0", "verdict": "supported"}] },
                    { "id": "c2", "text": "An unsupported aside", "verdict": "unsupported",
                      "citations": [{"block_id": "sec_1", "verdict": "unsupported"}] }
                ]
            }]
        })
        .to_string()
    }

    fn canonical_article() -> Article {
        use wiki_model::{Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION};
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 100,
                timestamp: "2026-11-02T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Moons".into(),
            display_title: "Moons".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Two moons orbit this planet.".into()),
            }],
            sections: vec![Section {
                ordinal: 1,
                heading: "Discussion".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Paragraph("An unsupported aside.".into()),
                }],
                subsections: vec![],
            }],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Moons</article>".into(),
            is_disambiguation: false,
        }
    }

    fn canonical_suite() -> String {
        suite()
            .replace("lead_0", "wkb:enwiki:42:100:b0")
            .replace("sec_1", "wkb:enwiki:42:100:1:b0")
    }

    #[test]
    fn strict_review_requires_real_revision_scoped_canonical_evidence() {
        let input = canonical_suite();
        let article = canonical_article();
        let report = evaluate_answer_support_with_corpus_json(&input, &[article.clone()]).unwrap();
        assert_eq!(report.totals.reviewed_citations, 2);
        let fabricated = input.replace("wkb:enwiki:42:100:b0", "wkb:enwiki:42:100:b99");
        assert_eq!(
            evaluate_answer_support_with_corpus_json(&fabricated, &[article.clone()]),
            Err(AnswerSupportError::UnknownEvidence("case_1".into()))
        );
        assert_eq!(
            evaluate_answer_support_with_corpus_json(&suite(), &[article.clone()]),
            Err(AnswerSupportError::UnknownEvidence("case_1".into()))
        );
        let mut stale = article.clone();
        stale.revision.revision_id = 101;
        assert_eq!(
            evaluate_answer_support_with_corpus_json(&input, &[stale]),
            Err(AnswerSupportError::UnknownOrStaleArticle("case_1".into()))
        );
        assert!(matches!(
            evaluate_answer_support_with_corpus_json(&input, &[article.clone(), article.clone()]),
            Err(AnswerSupportError::DuplicateCorpusArticle(_))
        ));
        assert_eq!(
            evaluate_answer_support_with_corpus_json(&input, &[]),
            Err(AnswerSupportError::InvalidCorpus)
        );
        let mut malformed = article.clone();
        malformed.revision.content_sha256 = "bad".into();
        assert!(matches!(
            evaluate_answer_support_with_corpus_json(&input, &[malformed]),
            Err(AnswerSupportError::InvalidCorpusArticle(_))
        ));
        let mut no_evidence = article;
        no_evidence.lead[0].content =
            wiki_model::BlockContent::HtmlFallback("<p>two moons</p>".into());
        assert_eq!(
            evaluate_answer_support_with_corpus_json(&input, &[no_evidence]),
            Err(AnswerSupportError::UnknownEvidence("case_1".into()))
        );
    }

    #[test]
    fn accepts_revision_scoped_canonical_block_ids() {
        let input = suite().replace("lead_0", "wkb:enwiki:42:100:b0");
        let report = evaluate_answer_support_json(&input).unwrap();
        assert_eq!(report.totals.supported_citations, 1);
    }

    #[test]
    fn canonical_block_citations_must_match_reviewed_revision_and_shape() {
        let valid = suite().replace("lead_0", "wkb:enwiki:42:100:0:2:b3");
        assert_eq!(
            evaluate_answer_support_json(&valid)
                .unwrap()
                .totals
                .supported_citations,
            1
        );
        for invalid in [
            "wkb:dewiki:42:100:b0",
            "wkb:enwiki:43:100:b0",
            "wkb:enwiki:42:101:b0",
            "wkb:enwiki:042:100:b0",
            "wkb:enwiki:42:0100:b0",
            "wkb:enwiki:42:100:00:b0",
            "wkb:enwiki:42:100:b00",
            "wkb:enwiki:42:100:4294967296:b0",
            "wkb:enwiki:42:100:b4294967296",
            "wkb:enwiki:42:100:b0:1",
            "wkb:enwiki:42:100:1",
            "other:enwiki:42:100:b0",
        ] {
            let input = suite().replace("lead_0", invalid);
            assert_eq!(
                evaluate_answer_support_json(&input).unwrap_err(),
                AnswerSupportError::InvalidCitation("case_1".into()),
                "{invalid}"
            );
        }
        let mut oversized: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        oversized["cases"][0]["revision_id"] = serde_json::json!(9223372036854775808u64);
        assert!(matches!(
            evaluate_answer_support_json(&oversized.to_string()),
            Err(AnswerSupportError::InvalidCase(_))
        ));
    }

    #[test]
    fn scores_reviewed_claims_and_citations_separately() {
        let report = evaluate_answer_support_json(&suite()).unwrap();
        assert_eq!(report.totals.claims, 2);
        assert_eq!(report.totals.reviewed_claims, 2);
        assert_eq!(report.totals.supported_claims, 1);
        assert_eq!(report.totals.reviewed_citations, 2);
        assert_eq!(report.totals.supported_citations, 1);
        assert!(report.all_claims_reviewed);
        assert!(report.all_citations_reviewed);
        assert_eq!(report.cases[0].answer_sha256.len(), 64);
        assert_eq!(report.input_sha256, sha256(suite().as_bytes()));
        serde_json::to_string(&report).unwrap();
    }

    #[test]
    fn claim_labels_must_reference_distinct_visible_answer_text() {
        let mut duplicated: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        let mut repeated = duplicated["cases"][0]["claims"][0].clone();
        repeated["id"] = serde_json::json!("another_id");
        duplicated["cases"][0]["claims"]
            .as_array_mut()
            .unwrap()
            .push(repeated);
        assert_eq!(
            evaluate_answer_support_json(&duplicated.to_string()),
            Err(AnswerSupportError::DuplicateClaim("case_1".into()))
        );

        // A marker's block ID is metadata, not generated answer prose.
        let mut marker_only: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        marker_only["cases"][0]["claims"][0]["text"] = serde_json::json!("lead_0");
        assert_eq!(
            evaluate_answer_support_json(&marker_only.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );

        // Removing a marker must not splice its neighbors into a new claim.
        let mut spliced: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        spliced["cases"][0]["answer"] =
            serde_json::json!("Two [[cite:lead_0]]moons. An unsupported aside [[cite:sec_1]].");
        spliced["cases"][0]["claims"][0]["text"] = serde_json::json!("Two moons");
        assert_eq!(
            evaluate_answer_support_json(&spliced.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );

        // Ordinary distinct claims still pass and retain their scores.
        assert_eq!(
            evaluate_answer_support_json(&suite())
                .unwrap()
                .totals
                .claims,
            2
        );
    }

    #[test]
    fn missing_unlabeled_and_duplicate_citations_fail_closed() {
        let s = suite();
        assert_eq!(
            evaluate_answer_support_json(&s.replace("[[cite:lead_0]]", "[[cite:changed]]"))
                .unwrap_err(),
            AnswerSupportError::MissingCitationMarker("case_1".into())
        );
        let missing = s.replace("\"block_id\":\"lead_0\"", "\"block_id\":\"other\"");
        assert_eq!(
            evaluate_answer_support_json(&missing).unwrap_err(),
            AnswerSupportError::MissingCitationMarker("case_1".into())
        );
        let extra = s.replace("[[cite:sec_1]]", "[[cite:sec_1]] [[cite:extra_2]]");
        assert_eq!(
            evaluate_answer_support_json(&extra).unwrap_err(),
            AnswerSupportError::UnlabeledCitationMarker("case_1".into())
        );
    }

    #[test]
    fn abstention_from_review_is_not_positive_evidence() {
        let s = suite().replace("\"verdict\":\"unsupported\"", "\"verdict\":\"unreviewed\"");
        let report = evaluate_answer_support_json(&s).unwrap();
        assert_eq!(report.totals.unreviewed_claims, 1);
        assert_eq!(report.totals.unreviewed_citations, 1);
        assert_eq!(report.totals.reviewed_claims, 1);
        assert!(!report.all_claims_reviewed);
        assert!(!report.all_citations_reviewed);
    }

    #[test]
    fn invalid_identity_unknown_field_and_inconsistent_support_rejected() {
        assert!(matches!(
            evaluate_answer_support_json(&suite().replace("\"page_id\":42", "\"page_id\":0")),
            Err(AnswerSupportError::InvalidCase(_))
        ));
        assert_eq!(
            evaluate_answer_support_json(
                &suite().replace("\"schema_version\":1", "\"schema_version\":2")
            )
            .unwrap_err(),
            AnswerSupportError::UnsupportedSchema(2)
        );
        assert_eq!(
            evaluate_answer_support_json(
                &suite().replace("\"id\":\"case_1\"", "\"id\":\"case_1\",\"extra\":1")
            )
            .unwrap_err(),
            AnswerSupportError::InvalidJson
        );
        assert!(matches!(
            evaluate_answer_support_json(&suite().replace(
                "\"block_id\":\"lead_0\",\"verdict\":\"supported\"",
                "\"block_id\":\"lead_0\",\"verdict\":\"unsupported\""
            )),
            Err(AnswerSupportError::InconsistentSupport(_))
        ));
    }

    #[test]
    fn bounded_and_nonvacuous() {
        assert_eq!(
            evaluate_answer_support_json(&"x".repeat(MAX_SUITE_BYTES + 1)).unwrap_err(),
            AnswerSupportError::TooLarge
        );
        assert!(matches!(
            evaluate_answer_support_json(r#"{"schema_version":1,"suite_id":"x","cases":[]}"#),
            Err(AnswerSupportError::InvalidCase(_))
        ));
    }
}
