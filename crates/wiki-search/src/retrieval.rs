//! RAG-001 current-article retrieval for chat context (spec §16).
//!
//! Ranks the blocks of one validated article revision with BM25 over block
//! text plus structural signals (heading match, the reader's current section,
//! and user-selected text), then fills a conservative context-token budget
//! with whole, exact blocks. Evidence is never truncated, because citations
//! must match supplied excerpts exactly (`validate_claimed_citations`).
//!
//! It answers conservatively: a request pinned to another revision gets
//! `RevisionMismatch`, and a query whose only matches are non-content terms
//! (present in every block of a non-trivial article, or one- or two-letter
//! alphabetic tokens) gets `NoEvidence` rather than arbitrary passages.
//! These are heuristics: an unrelated question that shares a content word
//! with the article still retrieves blocks. A lexical signal cannot prove a question answerable;
//! the answer-support validator still decides what a model may claim.

use std::collections::{BTreeMap, BTreeSet};

use crate::{evidence_word_cost, tokenize, tokenize_query, ArticleLexicalIndex, EvidenceHit};

/// BM25 parameters (Robertson/Spärck Jones defaults).
const K1: f64 = 1.2;
const B: f64 = 0.75;
/// A query term found in a block's heading path counts this much extra.
const HEADING_WEIGHT: f64 = 1.5;
/// Selection terms widen the query at reduced weight.
const SELECTION_TERM_WEIGHT: f64 = 0.5;
/// Matching blocks in the reader's current section are ranked up.
const CURRENT_SECTION_BOOST: f64 = 1.25;
/// Below this many blocks, "present in every block" says nothing.
const MIN_BLOCKS_FOR_UBIQUITY: usize = 3;
pub const MAX_RETRIEVED_BLOCKS: usize = 100;
pub const MAX_SELECTION_BYTES: usize = 8192;

/// What the caller knows about the reader when asking.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetrievalRequest {
    pub query: String,
    /// Revision the conversation is pinned to; must equal the index's.
    pub expected_revision: u64,
    /// Block the reader is currently viewing, if known.
    pub current_block_id: Option<String>,
    /// Text the user selected in the article, if any.
    pub selected_text: Option<String>,
    pub limit: usize,
    /// Upper bound on the estimated tokens of all returned evidence.
    pub max_context_tokens: usize,
}

/// Which structural signals contributed to a block's rank.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RankSignals {
    pub selection: bool,
    pub heading: bool,
    pub current_section: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievedBlock {
    /// Exact supplied evidence; `score` is the rank score in milli-units.
    pub hit: EvidenceHit,
    pub signals: RankSignals,
    pub estimated_tokens: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Retrieval {
    Evidence {
        blocks: Vec<RetrievedBlock>,
        used_tokens: usize,
        /// At least one ranked block was left out to respect the budget.
        budget_limited: bool,
    },
    /// Nothing discriminating matched: report unanswerable, do not guess.
    NoEvidence,
    /// The conversation is pinned to a different revision of the article.
    RevisionMismatch { expected: u64, actual: u64 },
    /// Empty or oversized query or selection, or a zero limit or budget.
    InvalidRequest,
}

/// Conservative context-token estimate for one evidence block, including
/// its heading path: the larger of ~4/3 tokens per word (CJK scalars count
/// as words) and one token per 4 UTF-8 bytes. Not a model tokenizer.
pub fn estimate_tokens(hit: &EvidenceHit) -> usize {
    let text = hit.heading_path.join(" / ") + "\n" + &hit.excerpt;
    let by_words = (evidence_word_cost(&text) * 4).div_ceil(3);
    let by_bytes = text.len().div_ceil(4);
    by_words.max(by_bytes).max(1)
}

fn normalize_for_match(text: &str) -> String {
    tokenize(text).join(" ")
}

impl ArticleLexicalIndex {
    /// Rank and budget this revision's blocks for a chat question.
    pub fn retrieve(&self, request: &RetrievalRequest) -> Retrieval {
        if request.expected_revision != self.revision_id {
            return Retrieval::RevisionMismatch {
                expected: request.expected_revision,
                actual: self.revision_id,
            };
        }
        let selection = request
            .selected_text
            .as_deref()
            .filter(|text| !text.trim().is_empty());
        if request.query.len() > crate::MAX_LEXICAL_QUERY_BYTES
            || selection.is_some_and(|text| text.len() > MAX_SELECTION_BYTES)
            || request.limit == 0
            || request.max_context_tokens == 0
        {
            return Retrieval::InvalidRequest;
        }
        let mut weights: BTreeMap<String, f64> = BTreeMap::new();
        for term in tokenize_query(&request.query).into_iter().take(32) {
            weights.insert(term, 1.0);
        }
        if let Some(text) = selection {
            for term in tokenize_query(text).into_iter().take(64) {
                weights.entry(term).or_insert(SELECTION_TERM_WEIGHT);
            }
        }
        if weights.is_empty() {
            return Retrieval::InvalidRequest;
        }

        let docs: Vec<Vec<String>> = self.entries.iter().map(|e| tokenize(&e.excerpt)).collect();
        let n = docs.len();
        if n == 0 {
            return Retrieval::NoEvidence;
        }
        let avg_len = docs.iter().map(Vec::len).sum::<usize>() as f64 / n as f64;
        let mut df: BTreeMap<&str, usize> = BTreeMap::new();
        for doc in &docs {
            for term in doc.iter().collect::<BTreeSet<_>>() {
                if weights.contains_key(term.as_str()) {
                    *df.entry(term.as_str()).or_default() += 1;
                }
            }
        }
        let idf = |term: &str| {
            let d = *df.get(term).unwrap_or(&0) as f64;
            (1.0 + (n as f64 - d + 0.5) / (d + 0.5)).ln()
        };
        // A match only counts towards answerability if the term is
        // content-bearing: not a one- or two-letter alphabetic token such as
        // "is" or "of" (CJK tokens are exempt from the length rule). A term
        // present in every block of a non-trivial article (like "the") is
        // discounted only when the question also has a content term that is
        // NOT everywhere; otherwise the question is about the article's
        // pervasive subject ("gravity" in Gravity) and those matches count.
        // Short and discounted terms still contribute to ranking.
        let content_bearing =
            |term: &str| term.chars().count() >= 3 || term.chars().any(crate::is_cjk);
        let ubiquitous =
            |term: &str| n >= MIN_BLOCKS_FOR_UBIQUITY && df.get(term).copied().unwrap_or(0) >= n;
        let has_specific_content_term = weights
            .keys()
            .any(|term| content_bearing(term) && !ubiquitous(term));
        let informative =
            |term: &str| content_bearing(term) && (!ubiquitous(term) || !has_specific_content_term);

        let current_section = request.current_block_id.as_deref().and_then(|id| {
            self.entries
                .iter()
                .find(|entry| entry.block_id == id)
                .map(|entry| entry.heading_path.clone())
        });
        let selection_norm = selection.map(normalize_for_match).filter(|s| !s.is_empty());

        let mut ranked = Vec::new();
        let mut any_informative = false;
        for (entry, doc) in self.entries.iter().zip(&docs) {
            let mut tf: BTreeMap<&str, usize> = BTreeMap::new();
            for term in doc {
                if weights.contains_key(term.as_str()) {
                    *tf.entry(term.as_str()).or_default() += 1;
                }
            }
            let heading_terms: BTreeSet<String> = entry
                .heading_path
                .iter()
                .flat_map(|heading| tokenize(heading))
                .collect();
            let mut signals = RankSignals::default();
            let mut score = 0.0;
            let mut matched_informative = false;
            for (term, weight) in &weights {
                let f = *tf.get(term.as_str()).unwrap_or(&0) as f64;
                let mut term_score = 0.0;
                if f > 0.0 {
                    let norm = K1 * (1.0 - B + B * doc.len() as f64 / avg_len.max(1.0));
                    term_score += idf(term) * f * (K1 + 1.0) / (f + norm);
                }
                if heading_terms.contains(term) {
                    term_score += HEADING_WEIGHT * idf(term);
                    signals.heading = true;
                }
                if term_score > 0.0 && informative(term) {
                    matched_informative = true;
                }
                score += weight * term_score;
            }
            if let Some(norm) = &selection_norm {
                if normalize_for_match(&entry.excerpt).contains(norm.as_str()) {
                    signals.selection = true;
                    matched_informative = true;
                }
            }
            if score <= 0.0 && !signals.selection {
                continue;
            }
            any_informative |= matched_informative;
            if current_section.as_ref() == Some(&entry.heading_path) {
                signals.current_section = true;
                score *= CURRENT_SECTION_BOOST;
            }
            let mut hit = entry.clone();
            hit.score = (score * 1000.0).round() as usize;
            ranked.push((signals, hit));
        }
        if !any_informative {
            return Retrieval::NoEvidence;
        }
        // Blocks containing the user's selection come first, then by score,
        // then by block ID for determinism.
        ranked.sort_by(|(sa, a), (sb, b)| {
            sb.selection
                .cmp(&sa.selection)
                .then_with(|| b.score.cmp(&a.score))
                .then_with(|| a.block_id.cmp(&b.block_id))
        });

        let limit = request.limit.min(MAX_RETRIEVED_BLOCKS);
        let mut blocks = Vec::new();
        let mut used_tokens = 0;
        let mut budget_limited = false;
        for (signals, hit) in ranked {
            if blocks.len() == limit {
                break;
            }
            let cost = estimate_tokens(&hit);
            if used_tokens + cost > request.max_context_tokens {
                budget_limited = true;
                continue;
            }
            used_tokens += cost;
            blocks.push(RetrievedBlock {
                hit,
                signals,
                estimated_tokens: cost,
            });
        }
        // Evidence that exists but does not fit whole is reported as
        // budget-limited with no blocks; it is never truncated.
        Retrieval::Evidence {
            blocks,
            used_tokens,
            budget_limited,
        }
    }
}
