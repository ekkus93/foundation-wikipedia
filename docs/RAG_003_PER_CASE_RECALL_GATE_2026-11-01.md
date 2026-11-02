# RAG-003 — Optional per-question retrieval recall gate

**Date:** 2026-11-01 (America/Los_Angeles)  
**Scope:** `crates/wiki-search/examples/retrieval_suite_runner.rs`  
**Status:** Scoped software improvement; **RAG-003 remains unchecked**.

## Problem and change

The offline retrieval regression gate previously evaluated a micro-averaged recall floor over all labeled gold passages. A strong result on a few questions could conceal an answerable question for which **none** of the labeled evidence was retrieved. The existing no-answer false-positive gate did not address this case.

The runner now accepts `--min-case-recall-per-mille N`, where `N` is an integer from 0 to 1000. The default is **0**, preserving the prior aggregate-only decision for existing CI and callers. When `N > 0`, each answerable case must individually satisfy `found_count / relevant_count >= N / 1000`; no-answer cases are excluded from that ratio. A positive floor on a suite with no answerable cases fails rather than passing vacuously.

The schema-v3 report now contains `case_recall_gate` with `min_case_recall_per_mille`, `answerable_cases`, `below_threshold_case_ids` and `passed`. The top-level `passed` requires **both** the existing aggregate/no-answer gate and this optional per-case gate. The report is still atomically persisted before a failing exit, so failures retain diagnostic evidence. The stable library schema-v2 report is unchanged.

## Example

```bash
cargo run -p wiki-search --example retrieval_suite_runner --locked -- \
  --snapshot-root /path/to/snapshot \
  --suite /path/to/labeled-suite.json \
  --sha <exact-40-hex-commit> \
  --output /path/to/new-report.json \
  --top-k 5 --min-recall-per-mille 800 \
  --min-case-recall-per-mille 500 --max-no-answer-fp 0
```

Two answerable questions with 2/2 and 0/1 gold passages retrieved have aggregate recall 2/3, but fail a 500-per-mille **per-question** floor. The report names the missed question. This is lexical retrieval recall, not answer support.

## Regression evidence

- Unit tests cover a masked per-case miss, the backward-compatible zero floor, a positive floor with only no-answer cases, invalid threshold values and duplicate CLI options.
- A real SQLite catalog + zstd shard fixture runs a labeled question with known gold but an unmatchable query. With the aggregate floor set to zero and the per-case floor at 1000, the runner exits with a regression failure **after writing** a schema-v3 report naming the missed case.
- Initial implementation `f034dfa5dd3bbc833ed268991f668e40b8e2dd7d` failed rustfmt in [Rust workspace 41385149811](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41385149811). Formatted `554e1f99906bf2b52ad2891bf3d1cf97a245eaa0` exposed a missing test argument in [Rust workspace 41385215378](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41385215378). Corrected `153e078804fb26821a1a89126a3aa0565b6e2099` passed Rust tests, then the added end-to-end regression required formatting at `1b287a371e32e6f5864c2e0c9a6fbf2c81fa64e0`. Final source: **`5c2b50f84ebb0b1e4bfaa4426c38fec92b671814`**.
- Final-source CI: [Rust workspace 41385723317](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41385723317) — passed; [Android Rust ABI 41385723308](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41385723308) — passed; [Platform shells 41385723376](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41385723376) — in progress when recorded.

## Limitations and next work

This feature does **not** authenticate caller-supplied corpus/labels or the caller-asserted commit SHA. It does not assess whether model-generated prose is factually entailed by passages, nor whether displayed citations support individual claims. Representative independently labeled Wikimedia questions, authenticated source snapshots, citation precision/answer-support grading, and real-device performance measurements are still required. Desktop article-grounded chat integration (AI-005) is also still outstanding.
