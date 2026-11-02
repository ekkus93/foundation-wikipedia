# RAG-003 — Optional per-question retrieval precision gate

**Date:** 2026-11-01 (America/Los_Angeles)  
**Qualified source SHA:** `23559fb4ed80b2c4bc4c4812ac7215b4800e627a`  
**Status:** Partial; RAG-003 remains unchecked.

## Motivation

The existing aggregate retrieval recall and optional per-question recall gates do not constrain how many **irrelevant passages** appear alongside retrieved gold evidence. An answerable query could retrieve one relevant passage and several irrelevant passages while satisfying both recall thresholds. Such noise consumes prompt budget and increases the opportunity for unsupported claims.

## Implementation

`crates/wiki-search/examples/retrieval_suite_runner.rs` now accepts `--min-case-precision-per-mille N` with an integer range of 0–1000. Default **0** preserves the previous benchmark gate behavior. For each answerable case, precision is `found_count / retrieved_count`, where `found_count` counts returned passages with an exact project/page/revision/block gold match and `retrieved_count` is the number of returned top-k passages. Integer cross-multiplication avoids floating-point rounding. An answerable question returning zero passages fails a positive floor. No-answer cases are evaluated by the existing false-positive gate, not by this answerable-case precision ratio; a positive floor on an all-no-answer suite fails rather than passing vacuously.

Schema-v3 reports add `case_precision_gate` with `min_case_precision_per_mille`, `answerable_cases`, `below_threshold_case_ids`, and `passed`. The top-level `passed` now combines the baseline aggregate/no-answer gate, optional per-question recall gate, and optional per-question precision gate. Reports continue to be atomically published even when a gate fails. The stable library schema-v2 score API is unchanged.

Example:

```bash
cargo run -p wiki-search --example retrieval_suite_runner --locked -- \
  --snapshot-root /path/to/snapshot --suite /path/to/labeled-suite.json \
  --sha <exact-40-hex-commit> --output /path/to/new-report.json \
  --top-k 5 --min-recall-per-mille 800 \
  --min-case-recall-per-mille 500 --min-case-precision-per-mille 500 \
  --max-no-answer-fp 0
```

## Tests and exact-commit CI

Deterministic Rust regression tests cover noisy answerable retrieval (1 relevant of 3 retrieved), clean retrieval, an answerable case with zero hits, an all-no-answer suite, default-zero compatibility, invalid threshold values, duplicate CLI flags, and impossible scored hit counts. The real SQLite catalog + zstd snapshot fixture exercises a failed positive precision gate and verifies that the complete failed report names the missed case.

Initial source `e9a6064c24e7c8c15e7f36394208f0512cd2212f` failed rustfmt only ([Rust workspace 41389992415](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41389992415)). Corrected exact source **`23559fb4ed80b2c4bc4c4812ac7215b4800e627a`** passed:

- [Rust workspace 41390160553](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41390160553) — passed (format, strict Clippy, Rust tests).
- [Platform shells 41390160617](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41390160617) — passed.
- [Android Rust ABI qualification 41390160576](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41390160576) — passed.

## Boundaries and remaining acceptance

The new metric is **retrieval precision against caller-supplied gold passages**, not a measure of whether generated prose is factually entailed by evidence or whether a generated citation supports its specific claim. It does not independently authenticate the source corpus, the labels, or the caller-asserted Git SHA. RAG-003 still requires representative independently labeled Wikimedia questions, verified source and label provenance, factual-support and citation-precision evaluation, and measured desktop/Android performance. The desktop AI-005 article-grounded chat UI still needs canonical article loading, explicit consent and inference wiring.
