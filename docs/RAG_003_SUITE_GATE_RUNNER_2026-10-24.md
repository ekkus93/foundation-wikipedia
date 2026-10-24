# RAG-003 — Versioned Retrieval Suite Gate Runner (2026-10-24)

## Scope

Added `wiki_search::evaluation_suite::run_retrieval_suite_json` as a single library call that accepts an exact `CorpusLexicalIndex`, a version-1 JSON evaluation suite, `top_k`, a caller-asserted 40-character source Git commit SHA, and `RetrievalThresholds`. It returns a `SuiteRun` with a typed `passed` boolean and a stable, versioned `report_json` string.

The runner composes the existing strict `parse_retrieval_suite_json` and `evaluate_retrieval_report_json` contracts. It preserves exact revision-bound gold verification, deterministic case ordering, integer recall thresholds and no-answer false-positive policy. Errors distinguish suite parsing from evaluation/report failures. The typed pass/fail result allows a future CI adapter to fail a job without interpreting free-form log output.

## Synthetic acceptance coverage

- A versioned suite with one revision-scoped physics gold block and one no-answer case passes against a matching synthetic indexed article.
- Changing the no-answer query to a term that retrieves an article yields a **failed** report and `passed: false` rather than an error or a false pass.
- Changing a gold revision to a nonexistent revision returns `UnknownOrStaleGold`, not a misleading recall score.
- Invalid caller-asserted commit metadata and malformed suite JSON fail closed.

## Exact-head evidence

Initial implementation `1c5f14d8cdc859d24ef78bd2824496c4930c4673` failed formatting in [Rust workspace #40099172281](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40099172281). Corrected source `32f2b0f134714edec84eade6ffeb8a6613952393` passed:

- [Rust workspace #40099214503](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40099214503) — rustfmt, strict Clippy and workspace tests.
- [Platform shells #40099214487](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40099214487).
- [Android Rust ABI #40099214557](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40099214557).

## Limitations and next work

The input labels and source SHA remain **caller asserted**. A matching block ID/revision establishes structural provenance only, not whether the passage truly supports an answer. This library API does not load installed packs, authenticate Wikimedia source generations, grade factual entailment, invoke a CI job, persist production reports or benchmark physical devices.

RAG-003 remains unchecked. Next work should bind a representative, independently reviewed question/answer/evidence corpus to a reproducible verified pack snapshot; add a CLI/CI adapter that emits and archives the report and fails on the typed gate; then separately qualify citation support, retrieval relevance and desktop/Android performance.
