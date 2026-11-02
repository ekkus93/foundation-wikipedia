# RAG-003 — Canonical human-reviewed citation identity fence

**Date:** 2026-11-02 (America/Los_Angeles)  
**Exact source SHA:** `02a9ea7475197a6eff929bf52990a1e3c181ca72`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem and implementation

The human-reviewed answer-support evaluator previously checked that a literal `[[cite:...]]` marker matched a reviewer annotation, but not whether a canonical `wkb:` ID belonged to the reviewed case's article and revision. For example, an answer for `enwiki/42/revision 100` could carry `wkb:enwiki:43:100:b0` and be positively labeled.

`crates/wiki-search/src/answer_support.rs` now checks colon-bearing block IDs against the exact case identity. Required shape: `wkb:PROJECT:PAGE:REVISION[:SECTION...]:bORDINAL`, with canonical decimal `u32` section/block ordinals. Cross-project/page/revision IDs, leading-zero spellings, out-of-range ordinals, invalid suffixes and alternate prefixes fail closed. Existing single-token synthetic fixture IDs remain supported. Reviewed case revision IDs are now constrained to positive SQLite-safe signed 64-bit values.

Regression covers valid nested section paths, foreign project/page/revision, leading-zero IDs, overflow, malformed block suffixes, misplaced suffixes and revision IDs above `i64::MAX`.

## Exact-head verification

- [Rust workspace 41416283269](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41416283269): **passed** (format, strict Clippy, workspace tests).
- [Platform shells 41416283313](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41416283313): **passed**.
- [Android Rust ABI qualification 41416283300](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41416283300): **passed**.

The initial code commit `9c3b84a3a113cf6cae4e52278b00fafdac057cc3` had a malformed test string; the corrected `7c3ccc76bd2ca17c2dfd960bf9b17c101da5a371` failed rustfmt only ([Rust 41416151129](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41416151129)). The final exact source SHA above passed all three workflows. Neither earlier commit is acceptance evidence.

## Remaining qualification

This is citation *identity and syntax*, not membership in an authenticated canonical article evidence map, source provenance, reviewer independence or factual entailment. Legacy fixture IDs remain permitted, so production benchmarks must separately require canonical IDs and independently verify their membership in the pinned article revision. Continue RAG-003 with independently reviewed Wikimedia questions, source and annotation hashes, representative answer-support baselines and real-device performance tests.
