# RAG-003 — Revision-Scoped Lexical Retrieval Evaluation Harness (2026-10-24)

## Scope

Added `crates/wiki-search/src/evaluation.rs`, exported as `wiki_search::evaluation`. Its `evaluate_retrieval(index, cases, top_k)` API scores a bounded labeled suite against the shared `CorpusLexicalIndex`, with optional per-case project scope enforced **before** ranking. Positive cases supply exact project/page/revision/block gold identities; negative cases use empty gold and count unexpected retrieval as no-answer false positives.

The evaluator refuses empty or oversized suites, invalid top-k, duplicate/invalid case IDs, invalid queries/projects, more than 128 gold blocks per case, duplicate gold, out-of-scope gold and gold whose block identity is missing or stale in the exact indexed article revision. Results sort cases by stable ID and expose integer numerators/denominators: relevant gold total, found gold total, retrieved hit total and no-answer false-positive case count. Consumers can calculate recall-at-k and precision-at-k without non-deterministic floating-point rounding.

A deterministic synthetic fixture covers Physics/Mathematics questions, unanswerable queries with zero hits, negative queries with lexical matches, stale gold, duplicate gold, project scope and case-order invariance. This harness evaluates retrieval only; it does **not** establish factual entailment or validate model-written answers.

## Exact-head qualification

- Qualified source SHA: `b03d51c1199da1c414f9b6c023cfcd027618186c`.
- [Rust workspace #40087167024](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40087167024): passed format, strict Clippy and tests.
- [Platform shells #40087167030](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40087167030): passed.
- [Android Rust ABI qualification #40087167036](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40087167036): passed.
- Earlier attempts: `2f6be2db3dec1fa9b5588eb752e6cf6fc3d20258` failed rustfmt ([Rust #40086958034](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40086958034)); `972eb77e67edc1fbfb47b741ccc8261e7ffd5549` failed test fixtures with incorrect lead-block gold IDs ([Rust #40087032759](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40087032759)). Both issues were corrected and exact-head CI passed.

## Remaining qualification

The current fixtures are intentionally tiny and synthetic. RAG-003 still needs a representative, versioned labeled corpus covering factual, explanation, multi-section, ambiguous and no-answer questions; reproducible machine-readable score artifacts and thresholds; answer-support and citation-precision evaluation; regression gates on actual offline pack data; and device performance measurements. RAG-001/RAG-002 and the dependent RAG-003 acceptance remain open.
