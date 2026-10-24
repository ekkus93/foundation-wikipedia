# RAG-003 — Versioned Lexical Retrieval Report and Regression Thresholds (2026-10-24)

## Scope

Added `wiki_search::evaluation::evaluate_retrieval_report_json` to produce stable, pretty-printed, newline-terminated JSON for a bounded labeled lexical-retrieval suite. The v1 report includes a suite ID, a normalized **caller-claimed** 40-character hexadecimal source commit, deterministic per-case and aggregate integer retrieval counts, an explicit recall-per-thousand minimum and a maximum no-answer false-positive case count. The result records `passed` without floating-point rounding. A suite with no relevant gold cannot satisfy a positive recall floor.

The existing `evaluate_retrieval` API and its provenance/revision checks are unchanged. The report rejects invalid suite identifiers, malformed SHA strings and recall thresholds above 1000. It reuses the evaluator's bounded case, top-k and gold validation. It does not validate the commit against GitHub or authenticate the corpus; that remains the caller/CI pipeline's responsibility.

## Reproducible synthetic baseline

The checked-in `crates/wiki-search/fixtures/retrieval_report_v1.json` records the Physics/Mathematics/no-answer synthetic suite at k=2. It intentionally **fails** a strict no-answer false-positive threshold despite full gold recall. Unit tests compare generated output byte-for-byte to this baseline, confirm case-order invariance, reject invalid metadata/policy and ensure an empty-gold suite cannot pass positive recall requirements.

To use with a real labeled corpus, build a `CorpusLexicalIndex`, supply revision-scoped `RetrievalCase` entries and a `RetrievalThresholds` policy, then persist the returned JSON alongside the independently verified CI commit and source-generation receipt. This API is a diagnostic/gating primitive; it is not yet a CLI command or a production relevance gate.

## Exact-head qualification

- Initial implementation `4379783056b22f1c03dbcedb392b364f206ae70b` failed Rust formatting ([Rust #40090788386](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40090788386)).
- Rustfmt correction `a1775f025099d0ab16c4b0c45d9eef1eeafcce53` failed `--locked` because the new direct serde dependencies were absent from `Cargo.lock` ([Rust #40090843436](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40090843436)).
- Corrected source and lockfile SHA **`9d49004e5c06b15a8b084d66fed022ff49862386`** passed [Rust workspace #40090920909](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40090920909), [Platform shells #40090920942](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40090920942) and [Android Rust ABI #40090920929](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40090920929).

## Remaining qualification

RAG-003 is **not complete**. The baseline is synthetic and intentionally small. Still required: independently sourced versioned evaluation corpora with factual, explanatory, multi-section, selection, ambiguity and no-answer questions; source-backed factual answer-support/citation-precision grading; persisted reports for representative real offline packs; regression thresholds wired to CI; and Android/desktop performance measurements. No claim of publisher trust, complete offline media or factual entailment is made.
