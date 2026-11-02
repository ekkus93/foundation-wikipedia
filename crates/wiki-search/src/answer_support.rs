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
    /// Schema v2: exact UTF-8 byte span of this claim in the original answer.
    start_byte: Option<usize>,
    end_byte: Option<usize>,
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
    UnreviewedProse(String),
    DuplicateClaim(String),
    InvalidCitation(String),
    DuplicateCitation(String),
    MissingCitationMarker(String),
    MisplacedCitationMarker(String),
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
    /// Version of this report's serialization contract.
    pub schema_version: u32,
    /// Version of the reviewed input annotations; v3 enforces complete visible prose coverage.
    pub annotation_schema_version: u32,
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
#[derive(Debug)]
struct MarkerOccurrence {
    block_id: String,
    start: usize,
    end: usize,
}

fn answer_markers(
    answer: &str,
    case_id: &str,
) -> Result<(BTreeSet<String>, String, Vec<MarkerOccurrence>), AnswerSupportError> {
    let mut markers = BTreeSet::new();
    let mut occurrences = Vec::new();
    // Claims must be anchored in answer prose, not in citation IDs. Keep a
    // separator where a marker was so words on either side cannot join.
    let mut visible_answer = String::with_capacity(answer.len());
    let mut rest = answer;
    while let Some(start) = rest.find("[[cite:") {
        let marker_start = answer.len() - rest.len() + start;
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
        occurrences.push(MarkerOccurrence {
            block_id: id.to_owned(),
            start: marker_start,
            end: marker_start + "[[cite:".len() + end + 2,
        });
        rest = &rest[end + 2..];
    }
    visible_answer.push_str(rest);
    Ok((markers, visible_answer, occurrences))
}

/// Require reviewed literal claims to align with complete Unicode word tokens,
/// rather than matching a fragment inside a longer visible word. This is a
/// structural check, not a judgment about factual entailment.
fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn claim_has_word_boundaries(answer: &str, start: usize, end: usize) -> bool {
    let Some(claim) = answer.get(start..end) else {
        return false;
    };
    let left_is_boundary = !claim.chars().next().is_some_and(is_word_character)
        || !answer[..start]
            .chars()
            .next_back()
            .is_some_and(is_word_character);
    let right_is_boundary = !claim.chars().next_back().is_some_and(is_word_character)
        || !answer[end..].chars().next().is_some_and(is_word_character);
    left_is_boundary && right_is_boundary
}

fn visible_claim_occurs(answer: &str, claim: &str) -> bool {
    answer
        .match_indices(claim)
        .any(|(start, _)| claim_has_word_boundaries(answer, start, start + claim.len()))
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
    if !matches!(raw.schema_version, 1..=3) {
        return Err(AnswerSupportError::UnsupportedSchema(raw.schema_version));
    }
    if !valid_id(&raw.suite_id, 128) {
        return Err(AnswerSupportError::InvalidSuiteId);
    }
    if raw.cases.is_empty() || raw.cases.len() > MAX_CASES {
        return Err(AnswerSupportError::InvalidCase("suite case count".into()));
    }

    let mut seen_cases = BTreeSet::new();
    // Distinct IDs and whitespace-only presentation changes must not replay
    // the same question/answer/revision into aggregate quality scores.
    // This is NOT semantic paraphrase detection; it preserves exact words.
    let mut seen_case_payloads = BTreeSet::new();
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
        // Citation identifiers are not independent answer content. The same
        // question and visible answer cannot be counted twice merely because
        // a reviewer changed the cited block IDs in the second case.
        let (markers, visible_answer, occurrences) = answer_markers(&case.answer, &case.id)?;
        if !seen_case_payloads.insert((
            case.project.clone(),
            case.page_id,
            case.revision_id,
            case.question
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            visible_answer
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        )) {
            return Err(AnswerSupportError::DuplicateCase(case.id));
        }
        // Schema v2/v3 anchor every claim to an exact, disjoint UTF-8 byte span.
        // Citations must occur after that span and before the next claim.
        // This prevents unrelated citation markers elsewhere in an answer from
        // being credited to the reviewed assertion. V1 remains exploratory.
        let mut next_claim_start = BTreeMap::new();
        if raw.schema_version >= 2 {
            let mut spans = Vec::with_capacity(case.claims.len());
            for claim in &case.claims {
                let (Some(start), Some(end)) = (claim.start_byte, claim.end_byte) else {
                    return Err(AnswerSupportError::InvalidClaim(case.id));
                };
                if start >= end
                    || case.answer.get(start..end) != Some(claim.text.as_str())
                    || occurrences
                        .iter()
                        .any(|marker| start < marker.end && marker.start < end)
                    // Check the *annotated* occurrence, not an unrelated
                    // matching phrase elsewhere in the visible answer.
                    || !claim_has_word_boundaries(&case.answer, start, end)
                {
                    return Err(AnswerSupportError::InvalidClaim(case.id));
                }
                spans.push((start, end, claim.id.clone()));
            }
            spans.sort_by_key(|span| span.0);
            for pair in spans.windows(2) {
                if pair[0].1 > pair[1].0 {
                    return Err(AnswerSupportError::InvalidClaim(case.id));
                }
            }
            // Schema v3 additionally requires every visible word character to
            // belong to a reviewed claim. Punctuation/whitespace may remain
            // outside claims, but an unreviewed factual phrase cannot be
            // hidden between two annotated claims or after the last citation.
            // Citation marker bytes are excluded from the visible prose.
            if raw.schema_version == 3 {
                let mut covered = vec![false; case.answer.len()];
                for marker in &occurrences {
                    covered[marker.start..marker.end].fill(true);
                }
                for (start, end, _) in &spans {
                    covered[*start..*end].fill(true);
                }
                if case
                    .answer
                    .char_indices()
                    .any(|(offset, ch)| is_word_character(ch) && !covered[offset])
                {
                    return Err(AnswerSupportError::UnreviewedProse(case.id));
                }
            }
            for (index, (_, _, id)) in spans.iter().enumerate() {
                let next = spans
                    .get(index + 1)
                    .map_or(case.answer.len(), |span| span.0);
                next_claim_start.insert(id.clone(), next);
            }
        } else if case
            .claims
            .iter()
            .any(|claim| claim.start_byte.is_some() || claim.end_byte.is_some())
        {
            return Err(AnswerSupportError::InvalidClaim(case.id));
        }
        let mut assigned_occurrences = BTreeSet::new();
        let mut labeled_markers = BTreeSet::new();
        let mut seen_claims = BTreeSet::new();
        // Text-only claim annotations have no occurrence offsets. Counting
        // the same literal span under different IDs inflates support scores.
        let mut seen_claim_texts = BTreeSet::new();
        let mut counts = SupportCounts::default();

        for claim in case.claims {
            if !valid_id(&claim.id, 255)
                || claim.text.trim().is_empty()
                // A punctuation/emoji-only annotation is not a factual claim.
                // Otherwise it can pad the supported-claim denominator and
                // conceal unsupported prose in aggregate quality scores.
                || !claim.text.chars().any(is_word_character)
                || claim.text.len() > 8_192
                || (raw.schema_version == 1 && !visible_claim_occurs(&visible_answer, &claim.text))
                || claim.citations.len() > MAX_CITATIONS_PER_CLAIM
            {
                return Err(AnswerSupportError::InvalidClaim(case.id));
            }
            if !seen_claims.insert(claim.id.clone())
                || (raw.schema_version == 1 && !seen_claim_texts.insert(claim.text.clone()))
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
                if raw.schema_version >= 2 {
                    let end = claim.end_byte.expect("v2 span checked above");
                    let next = next_claim_start[&claim.id];
                    let mut found = false;
                    for (index, marker) in occurrences.iter().enumerate() {
                        if marker.block_id == citation.block_id
                            && marker.start >= end
                            && marker.end <= next
                        {
                            assigned_occurrences.insert(index);
                            found = true;
                        }
                    }
                    if !found {
                        return Err(AnswerSupportError::MisplacedCitationMarker(case.id));
                    }
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
                    // A reviewer cannot label a claim unsupported while also
                    // declaring that one of its cited blocks supports it.
                    if supported_citation {
                        return Err(AnswerSupportError::InconsistentSupport(case.id));
                    }
                    counts.unsupported_claims += 1;
                    counts.reviewed_claims += 1;
                }
                ReviewVerdict::Unreviewed => counts.unreviewed_claims += 1,
            }
        }
        if markers != labeled_markers
            || (raw.schema_version >= 2 && assigned_occurrences.len() != occurrences.len())
        {
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
        annotation_schema_version: raw.schema_version,
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
    fn duplicate_question_answer_revision_cannot_inflate_benchmark_scores() {
        for version in [1, 2, 3] {
            let mut data = if version == 1 {
                serde_json::from_str::<serde_json::Value>(&suite()).unwrap()
            } else {
                suite_v2()
            };
            data["schema_version"] = serde_json::json!(version);
            let mut replay = data["cases"][0].clone();
            replay["id"] = serde_json::json!("replay_with_different_id");
            data["cases"].as_array_mut().unwrap().push(replay);
            assert_eq!(
                evaluate_answer_support_json(&data.to_string()),
                Err(AnswerSupportError::DuplicateCase(
                    "replay_with_different_id".into()
                )),
                "schema v{version}"
            );

            // Reusing a question for a genuinely different answer or exact
            // revision is not a duplicate evaluation case.
            data["cases"][1]["answer"] = serde_json::json!(
                "Six moons [[cite:lead_0]]. An unsupported aside [[cite:sec_1]]."
            );
            data["cases"][1]["claims"][0]["text"] = serde_json::json!("Six moons");
            assert!(evaluate_answer_support_json(&data.to_string()).is_ok());

            let original = data["cases"][0].clone();
            data["cases"][1] = original;
            data["cases"][1]["id"] = serde_json::json!("replay_with_different_id");
            data["cases"][1]["revision_id"] = serde_json::json!(101);
            assert!(evaluate_answer_support_json(&data.to_string()).is_ok());
        }
    }

    #[test]
    fn whitespace_only_case_replays_cannot_inflate_scores() {
        for version in [1, 2, 3] {
            let mut data = if version == 1 {
                serde_json::from_str::<serde_json::Value>(&suite()).unwrap()
            } else {
                suite_v2()
            };
            data["schema_version"] = serde_json::json!(version);
            let mut replay = data["cases"][0].clone();
            replay["id"] = serde_json::json!("replay_whitespace");
            replay["question"] = serde_json::json!("  How   many\tmoons?\n");
            replay["answer"] =
                serde_json::json!(format!("{}  \n", replay["answer"].as_str().unwrap()));
            data["cases"].as_array_mut().unwrap().push(replay);
            assert_eq!(
                evaluate_answer_support_json(&data.to_string()),
                Err(AnswerSupportError::DuplicateCase(
                    "replay_whitespace".into()
                )),
                "schema v{version}"
            );

            // A different question remains an independent evaluation case.
            data["cases"][1]["question"] = serde_json::json!("How many planets?");
            assert!(evaluate_answer_support_json(&data.to_string()).is_ok());
        }
    }

    #[test]
    fn citation_id_changes_cannot_replay_the_same_visible_answer() {
        for version in [1, 2, 3] {
            let mut data = if version == 1 {
                serde_json::from_str::<serde_json::Value>(&suite()).unwrap()
            } else {
                suite_v2()
            };
            data["schema_version"] = serde_json::json!(version);
            let mut replay = data["cases"][0].clone();
            replay["id"] = serde_json::json!("replay_changed_citations");
            let changed_answer = replay["answer"]
                .as_str()
                .unwrap()
                .replace("lead_0", "lead_1")
                .replace("sec_1", "sec_2");
            replay["answer"] = serde_json::json!(changed_answer);
            for claim in replay["claims"].as_array_mut().unwrap() {
                let changed_id = claim["citations"][0]["block_id"]
                    .as_str()
                    .unwrap()
                    .replace("lead_0", "lead_1")
                    .replace("sec_1", "sec_2");
                claim["citations"][0]["block_id"] = serde_json::json!(changed_id);
            }
            // Each case is structurally valid by itself. It is the replay
            // of the same reviewed prose that must fail the aggregate suite.
            let mut single = data.clone();
            single["cases"] = serde_json::json!([replay.clone()]);
            assert!(
                evaluate_answer_support_json(&single.to_string()).is_ok(),
                "schema v{version}"
            );
            data["cases"].as_array_mut().unwrap().push(replay);
            assert_eq!(
                evaluate_answer_support_json(&data.to_string()),
                Err(AnswerSupportError::DuplicateCase(
                    "replay_changed_citations".into()
                )),
                "schema v{version}"
            );
        }
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
    fn reviewed_claims_cannot_be_substrings_of_longer_words() {
        let mut data: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("moon");
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("moons");
        assert!(evaluate_answer_support_json(&data.to_string()).is_ok());

        // Apply the same boundary rule to Unicode letters, not just ASCII.
        data["cases"][0]["answer"] =
            serde_json::json!("The cafés [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].");
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("café");
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("cafés");
        assert!(evaluate_answer_support_json(&data.to_string()).is_ok());
    }

    #[test]
    fn unsupported_claim_cannot_have_a_supported_citation() {
        let mut data: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        data["cases"][0]["claims"][1]["citations"][0]["verdict"] = serde_json::json!("supported");
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InconsistentSupport("case_1".into()))
        );
        // A supported claim still requires a supported citation.
        data["cases"][0]["claims"][1]["verdict"] = serde_json::json!("supported");
        assert!(evaluate_answer_support_json(&data.to_string()).is_ok());
    }

    fn suite_v2() -> serde_json::Value {
        let mut data: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        data["schema_version"] = serde_json::json!(2);
        let answer = data["cases"][0]["answer"].as_str().unwrap().to_owned();
        for claim in data["cases"][0]["claims"].as_array_mut().unwrap() {
            let text = claim["text"].as_str().unwrap().to_owned();
            let start = answer.find(&text).unwrap();
            claim["start_byte"] = serde_json::json!(start);
            claim["end_byte"] = serde_json::json!(start + text.len());
        }
        data
    }

    #[test]
    fn nonlexical_claims_cannot_pad_reviewed_support_scores() {
        let mut legacy: serde_json::Value = serde_json::from_str(&suite()).unwrap();
        legacy["cases"][0]["answer"] =
            serde_json::json!("! [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].");
        legacy["cases"][0]["claims"][0]["text"] = serde_json::json!("!");
        assert_eq!(
            evaluate_answer_support_json(&legacy.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );

        // Schema v3 covers all prose, but a punctuation-only extra claim
        // must not inflate the numerator or denominator of the quality gate.
        let mut strict = suite_v2();
        strict["schema_version"] = serde_json::json!(3);
        let answer = format!(
            "{} ! [[cite:lead_0]]",
            strict["cases"][0]["answer"].as_str().unwrap()
        );
        let start = answer.rfind("! [[cite:lead_0]]").unwrap();
        strict["cases"][0]["answer"] = serde_json::json!(answer);
        let mut padding = strict["cases"][0]["claims"][0].clone();
        padding["id"] = serde_json::json!("padding");
        padding["text"] = serde_json::json!("!");
        padding["start_byte"] = serde_json::json!(start);
        padding["end_byte"] = serde_json::json!(start + 1);
        strict["cases"][0]["claims"]
            .as_array_mut()
            .unwrap()
            .push(padding);
        assert_eq!(
            evaluate_answer_support_json(&strict.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );

        // A non-Latin lexical claim remains valid.
        legacy["cases"][0]["answer"] =
            serde_json::json!("月 [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].");
        legacy["cases"][0]["claims"][0]["text"] = serde_json::json!("月");
        assert!(evaluate_answer_support_json(&legacy.to_string()).is_ok());
    }

    #[test]
    fn v3_requires_complete_visible_word_coverage() {
        let mut data = suite_v2();
        data["schema_version"] = serde_json::json!(3);
        let report = evaluate_answer_support_json(&data.to_string()).unwrap();
        assert_eq!(report.annotation_schema_version, 3);

        let answer = data["cases"][0]["answer"].as_str().unwrap().to_owned();
        data["cases"][0]["answer"] = serde_json::json!(format!("{answer} Extra claim."));
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::UnreviewedProse("case_1".into()))
        );
        data["cases"][0]["answer"] = serde_json::json!(format!("{answer} café"));
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::UnreviewedProse("case_1".into()))
        );

        // Non-lexical punctuation is not a factual claim.
        data["cases"][0]["answer"] = serde_json::json!(format!("{answer} !?"));
        assert!(evaluate_answer_support_json(&data.to_string()).is_ok());

        // Preserve schema-v2's explicitly partial-annotation behavior.
        data["schema_version"] = serde_json::json!(2);
        data["cases"][0]["answer"] = serde_json::json!(format!("{answer} Extra claim."));
        assert!(evaluate_answer_support_json(&data.to_string()).is_ok());
    }

    #[test]
    fn v2_claim_spans_bind_citations_to_their_own_occurrences() {
        let mut data = suite_v2();
        let report = evaluate_answer_support_json(&data.to_string()).unwrap();
        assert_eq!(report.totals.supported_claims, 1);

        // Swapping annotations still leaves both IDs present in the answer,
        // but neither citation is adjacent to its own claim.
        data["cases"][0]["claims"][0]["citations"][0]["block_id"] = serde_json::json!("sec_1");
        data["cases"][0]["claims"][1]["citations"][0]["block_id"] = serde_json::json!("lead_0");
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::MisplacedCitationMarker("case_1".into()))
        );
    }

    #[test]
    fn v2_rejects_overlaps_missing_offsets_and_marker_only_spans() {
        let mut data = suite_v2();
        data["cases"][0]["claims"][0]
            .as_object_mut()
            .unwrap()
            .remove("start_byte");
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
        data = suite_v2();
        let mut overlap = data["cases"][0]["claims"][0].clone();
        overlap["id"] = serde_json::json!("overlap");
        overlap["text"] = serde_json::json!("moons");
        let answer = data["cases"][0]["answer"].as_str().unwrap();
        let start = answer.find("moons").unwrap();
        overlap["start_byte"] = serde_json::json!(start);
        overlap["end_byte"] = serde_json::json!(start + "moons".len());
        data["cases"][0]["claims"]
            .as_array_mut()
            .unwrap()
            .push(overlap);
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
        data = suite_v2();
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("lead_0");
        let answer = data["cases"][0]["answer"].as_str().unwrap();
        let start = answer.find("lead_0").unwrap();
        data["cases"][0]["claims"][0]["start_byte"] = serde_json::json!(start);
        data["cases"][0]["claims"][0]["end_byte"] = serde_json::json!(start + 6);
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
    }

    #[test]
    fn v2_word_boundaries_must_hold_at_the_annotated_span() {
        let mut data = suite_v2();
        let answer = "Two moons. moon [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].";
        data["cases"][0]["answer"] = serde_json::json!(answer);
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("moon");
        let first = answer.find("moon").unwrap();
        data["cases"][0]["claims"][0]["start_byte"] = serde_json::json!(first);
        data["cases"][0]["claims"][0]["end_byte"] = serde_json::json!(first + 4);
        let aside = answer.find("An unsupported aside").unwrap();
        data["cases"][0]["claims"][1]["start_byte"] = serde_json::json!(aside);
        data["cases"][0]["claims"][1]["end_byte"] =
            serde_json::json!(aside + "An unsupported aside".len());
        // The same word occurs independently later in the answer. That
        // occurrence must not launder a partial-word annotated span.
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
        let standalone = answer.find(" moon [[cite:").unwrap() + 1;
        data["cases"][0]["claims"][0]["start_byte"] = serde_json::json!(standalone);
        data["cases"][0]["claims"][0]["end_byte"] = serde_json::json!(standalone + 4);
        assert!(evaluate_answer_support_json(&data.to_string()).is_ok());

        // Check a partial prefix, and a Unicode suffix, against a separate
        // complete occurrence of the same literal claim.
        for (answer, fragment, expected) in [
            (
                "premoon. moon [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].",
                "moon",
                3,
            ),
            (
                "cafés. café [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].",
                "café",
                0,
            ),
        ] {
            data["cases"][0]["answer"] = serde_json::json!(answer);
            data["cases"][0]["claims"][0]["text"] = serde_json::json!(fragment);
            data["cases"][0]["claims"][0]["start_byte"] = serde_json::json!(expected);
            data["cases"][0]["claims"][0]["end_byte"] =
                serde_json::json!(expected + fragment.len());
            let aside = answer.find("An unsupported aside").unwrap();
            data["cases"][0]["claims"][1]["start_byte"] = serde_json::json!(aside);
            data["cases"][0]["claims"][1]["end_byte"] =
                serde_json::json!(aside + "An unsupported aside".len());
            assert_eq!(
                evaluate_answer_support_json(&data.to_string()),
                Err(AnswerSupportError::InvalidClaim("case_1".into()))
            );
        }
    }

    #[test]
    fn v2_allows_repeated_literal_claims_at_distinct_spans() {
        let mut data = suite_v2();
        let original = data["cases"][0]["answer"].as_str().unwrap().to_owned();
        data["cases"][0]["answer"] =
            serde_json::json!(format!("{original} Two moons [[cite:lead_0]]."));
        let mut repeat = data["cases"][0]["claims"][0].clone();
        repeat["id"] = serde_json::json!("repeat");
        let start = data["cases"][0]["answer"]
            .as_str()
            .unwrap()
            .rfind("Two moons")
            .unwrap();
        repeat["start_byte"] = serde_json::json!(start);
        repeat["end_byte"] = serde_json::json!(start + "Two moons".len());
        data["cases"][0]["claims"]
            .as_array_mut()
            .unwrap()
            .push(repeat);
        let report = evaluate_answer_support_json(&data.to_string()).unwrap();
        assert_eq!(report.totals.claims, 3);
        assert_eq!(report.totals.supported_claims, 2);
        // V1 deliberately cannot disambiguate repeated text with different IDs.
        data["schema_version"] = serde_json::json!(1);
        for claim in data["cases"][0]["claims"].as_array_mut().unwrap() {
            claim.as_object_mut().unwrap().remove("start_byte");
            claim.as_object_mut().unwrap().remove("end_byte");
        }
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::DuplicateClaim("case_1".into()))
        );
    }

    #[test]
    fn v1_rejects_v2_offsets_and_v2_rejects_mid_utf8_spans() {
        let mut data = suite_v2();
        data["schema_version"] = serde_json::json!(1);
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
        );
        data = suite_v2();
        let answer = "Café [[cite:lead_0]]. An unsupported aside [[cite:sec_1]].";
        data["cases"][0]["answer"] = serde_json::json!(answer);
        data["cases"][0]["claims"][0]["text"] = serde_json::json!("Café");
        data["cases"][0]["claims"][0]["start_byte"] = serde_json::json!(0);
        data["cases"][0]["claims"][0]["end_byte"] = serde_json::json!(4);
        assert_eq!(
            evaluate_answer_support_json(&data.to_string()),
            Err(AnswerSupportError::InvalidClaim("case_1".into()))
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
                &suite().replace("\"schema_version\":1", "\"schema_version\":4")
            )
            .unwrap_err(),
            AnswerSupportError::UnsupportedSchema(4)
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
