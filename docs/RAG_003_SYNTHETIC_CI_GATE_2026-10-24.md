# RAG-003 — Synthetic CI Retrieval Gate (2026-10-24)

## Implementation

The Rust workspace GitHub Actions workflow now runs `cargo run -p wiki-search --example synthetic_retrieval_gate --locked` after Rust tests. The example builds a small, explicitly synthetic revision-scoped Physics corpus and reads the checked-in version-1 `crates/wiki-search/fixtures/retrieval_suite_v1.json`. It calls `run_retrieval_suite_json` with top-k 2, 100% recall and zero permitted false positives on the unanswerable query.

The example requires `GITHUB_SHA`, checks its syntax through the existing library report contract, writes `retrieval-synthetic-report.json` before checking the gate decision, and exits nonzero if the evaluation fails. The workflow retains the report in `rust-test-evidence` using an `if: always()` artifact-upload step. This ensures a regression decision is enforced by CI and its report can be inspected. The SHA is caller asserted; it is not independently authenticated by this example.

## Exact-head evidence

Source commit `a0805381627b224066a0af6ad47f2646bfd4c130`:

- [Rust workspace #40103232660](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40103232660) — passed format, strict Clippy, tests, the synthetic retrieval regression gate, and artifact upload. The `rust-test-evidence` artifact was present.
- [Android Rust ABI #40103232671](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40103232671) — passed.
- [Platform shells #40103232653](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40103232653) — still in progress at documentation time.

## Remaining acceptance work

The corpus and labels are synthetic; this does **not** measure real Wikimedia relevance or factual answer support. RAG-003 still requires independently reviewed, representative Wikimedia question/answer/evidence labels tied to an authenticated reproducible pack snapshot; real-pack report persistence and thresholds; citation-support evaluation; and desktop/Android performance and relevance measurements. Do not mark RAG-003 complete on the strength of this gate.
