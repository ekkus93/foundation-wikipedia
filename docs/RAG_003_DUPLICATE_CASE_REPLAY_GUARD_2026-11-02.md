# RAG-003 — Duplicate reviewed-case replay guard (2026-11-02)

## Scope and defect

The human-reviewed answer-support benchmark previously required distinct `case.id` values but did not reject identical question/answer examples repeated under different case IDs. A supported example could therefore be duplicated to inflate aggregate claim-support and citation-precision metrics without adding independent evaluation coverage.

## Implementation

`crates/wiki-search/src/answer_support.rs` now rejects a repeated exact tuple of `(project, page_id, revision_id, question, answer)` with `AnswerSupportError::DuplicateCase`, independently of the case identifier and claim annotations. This applies to schema v1, v2 and v3. Legitimate alternative answers to the same question and identical question/answer text at a different exact revision are permitted. The implementation uses exact string comparison; it is not a semantic or whitespace-normalized near-duplicate detector.

The regression `duplicate_question_answer_revision_cannot_inflate_benchmark_scores` checks replay rejection and these two legitimate distinctions for each supported annotation schema.

## Exact-head verification

- Passing implementation SHA: `c5095f522f5e831d87e5bc3aca8c46d7fa9d77ee`.
- Rust workspace (format, Clippy, tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/41522538646 — passed.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41522538643 — passed.
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41522538650 — passed.
- Earlier `a13cd829f28cf5cae123d39427a915b199a600dd` failed formatting and was superseded; no acceptance is claimed for that SHA.

## Qualification limits

This is a benchmark score-integrity safeguard, not an independent factual-entailment verifier. It does not detect semantically equivalent paraphrased duplicates or maliciously modified answers. The source publication, human reviewer and judgments are not authenticated by this change. Representative independently reviewed Wikimedia datasets and real-device acceptance are still required. **RAG-003 remains unchecked.**
