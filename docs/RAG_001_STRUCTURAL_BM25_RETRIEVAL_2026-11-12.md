# RAG-001 — Structural BM25 current-article retrieval

**Date:** 2026-11-12  
**Repository:** `ekkus93/foundation-wikipedia`  
**Code SHA:** `8d4c78d8`

## Issue

Spec §16 asks current-article retrieval to rank provenance-linked blocks with "lexical + structural signals (section title, current location, selected text)", cap context tokens, and identify unanswerable questions instead of filling them. `ArticleLexicalIndex::search` scored a block by whether each query term was present (body hit +1, heading hit +4). It had no term weighting, no current-location or selection signal, only a word budget, and no notion of "no evidence" or of a request pinned to a different revision.

## Implementation (`crates/wiki-search/src/retrieval.rs`)

`ArticleLexicalIndex::retrieve(&RetrievalRequest) -> Retrieval`.

**Request.** It carries:
- the query;
- `expected_revision`, the revision the conversation is pinned to;
- an optional `current_block_id`, the block the reader is viewing;
- an optional `selected_text` (at most 8 KiB);
- `limit`, capped at 100;
- `max_context_tokens`.

**Ranking.**
- BM25 (k1 = 1.2, b = 0.75) over the article's own blocks, with IDF from block frequency.
- Each query term found in a block's heading path adds 1.5 × IDF.
- Terms from the selection join the query at half weight.
- Blocks whose normalized text contains the normalized selection are ranked first.
- Matching blocks in the reader's current section get a 1.25× boost. The boost never adds blocks that don't match.
- Ties break by block ID, so ranking is deterministic.

Each block reports which signals applied (`RankSignals`).

**Budget.** Ranked blocks are added whole while the estimated token total fits. A block that doesn't fit is skipped, never truncated, because citations must equal supplied excerpts exactly. `estimate_tokens` covers the heading path plus the text and takes the larger of ⌈4/3 × words⌉ (CJK scalars count as words) and ⌈UTF-8 bytes / 4⌉. It is a conservative heuristic, not a model tokenizer. `budget_limited` reports when anything was skipped.

**Conservative outcomes:**
- `RevisionMismatch { expected, actual }` when the pinned revision differs; no evidence is returned.
- `NoEvidence` when no block matches a content-bearing term. A term is content-bearing if it is not present in every block of an article with at least 3 blocks, and it is either at least 3 characters long or a CJK token. A selection match also counts. Short or ubiquitous terms still contribute to ranking.
- `InvalidRequest` for an empty or oversized query or selection, a zero limit or a zero budget.

The existing `search`, the corpus index and the CI retrieval gate are unchanged.

## Evidence

`crates/wiki-search/tests/current_article_retrieval.rs` (9 tests, synthetic 7-block article):

- BM25 puts the answering block first, and scores are non-increasing.
- A heading-only match retrieves both "Design" blocks, with the heading signal set.
- A selection pins its block first even when the query shares no terms with it.
- The current-section boost multiplies a matching block's score by exactly 1.25 and adds no blocks.
- Budgeting keeps whole, exact blocks within the budget and reports `budget_limited`. Retrieved blocks pass `validate_claimed_citations`. A 1-token budget returns no blocks and `budget_limited`.
- No-evidence cases: a question whose only matches are "the", which is in every block (`NoEvidence`), and an unrelated query (`NoEvidence`). A discriminating question still retrieves.
- A wrong pinned revision gives `RevisionMismatch`, and the five invalid requests are refused.
- Repeated calls return identical results.
- **Labeled set:** 7 answerable questions each rank their answer block first, and 2 no-answer questions abstain.

The labeled set exposed a real weakness in the first heuristic. "What is the boiling point of mercury?" retrieved blocks only because it shared "is" with them. The content-bearing rule fixed that, and the case is now a regression test.

Local verification (Rust 1.85.1, `umask 022`): fmt, clippy `-D warnings`, workspace tests, Python tests (644) and fixture provenance all passed. Exact-head CI on `8d4c78d8`: [Rust workspace 43383563260](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43383563260), [Platform shells 43383562923](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43383562923) and [Android Rust ABI 43383563015](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43383563015) passed (the Android codec probe does not run for `wiki-search`-only changes).

## Boundaries

- These are lexical heuristics on synthetic text, not a relevance benchmark on real Wikipedia. That is the job of RAG-003's labeled corpus.
- There is no stemming ("begin" does not match "began") and no language-specific stop words.
- An unrelated question that shares a content word with the article still retrieves blocks.
- Whether a model's answer is supported is decided by the answer-support validator, not by retrieval.
- Chat orchestration (AI-005) does not call `retrieve` yet.
- RAG-001's acceptance ("labeled Q&A retrieves relevant source blocks without a vector DB") is shown only on this small synthetic set. **RAG-001 stays unchecked.**
