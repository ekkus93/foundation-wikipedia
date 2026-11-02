# RAG-003 — Reviewed-answer benchmark coverage floors (2026-11-02)

## Gap addressed

A fully reviewed one-question, one-article suite could pass every existing claim-support, citation-precision, per-case and schema-v3 gate. Multiple questions or revisions of the same article also do not provide independent article diversity.

## Implementation

The `wiki-search --example answer_support_runner` executable now accepts optional `--min-cases N` and `--min-distinct-articles N`, each bounded to 0..1000 (default zero for compatibility). Distinct articles are identified by (Wikimedia project, page ID), ignoring revision and question count.

The report's `gate.coverage_gate` records `min_cases`, `min_distinct_articles`, `observed_cases`, `observed_distinct_articles`, and `passed`. It contributes to the overall `gate.passed`. Insufficient coverage exits with a failed-quality status **and publishes a no-clobber diagnostic report**; invalid/duplicate flags are rejected before scoring.

Example (operator supplies independently reviewed inputs and validates source provenance):

```sh
cargo run -p wiki-search --example answer_support_runner --locked -- \
  --suite reviewed_answers.json --corpus canonical_articles.json \
  --output reviewed_report.json --require-schema-v3 \
  --min-cases 100 --min-distinct-articles 25 \
  --min-claim-support-per-mille 800 \
  --min-citation-precision-per-mille 800
```

## Automated verification

Regression tests cover valid/invalid coverage flags, duplicate flags, a one-case strict canonical-corpus benchmark that passes normal support thresholds but fails the new coverage floors, and two questions that refer to just one distinct article.

Initial source `a3e17ed44ce0e3c8e6daef5c2945969f2f3f1787` failed Rust formatting ([Rust workspace 41548613521](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41548613521)); corrected source **`ec27931b63233b2aa0b0c186a1cf179a9a6bca05`** passed:

- [Rust workspace (fmt, Clippy, workspace tests, executable tests) — 41548703695](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41548703695).
- [Platform shells — 41548703594](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41548703594).
- [Android Rust ABI qualification — 41548703561](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41548703561).

## Limits and next work

Structural thresholds do **not** prove dataset representativeness, independent human review, Wikimedia publisher authenticity, source completeness, factual entailment or actual Android/desktop behavior. Independently reviewed, source-authenticated Wikimedia examples and real-device qualification remain outstanding. Synthetic tests are not represented as factual evidence. **RAG-003 remains unchecked.**
