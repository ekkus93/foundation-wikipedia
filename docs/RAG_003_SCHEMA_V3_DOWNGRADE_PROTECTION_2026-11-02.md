# RAG-003 — Schema-v3 reviewed-answer downgrade protection (2026-11-02)

## Scope and result

The answer-support evaluator already supported annotation schemas v1, v2 and v3. Only v3 requires every visible word character outside citation markers to be covered by a reviewer-annotated claim. A benchmark using v1/v2 could therefore obtain a high claim-support score while omitting unsupported prose. This increment makes the selected annotation policy explicit and enforceable in the executable benchmark.

- `AnswerSupportReport.annotation_schema_version` now records the annotation input version independently of the stable report `schema_version` (currently 1).
- `cargo run -p wiki-search --example answer_support_runner --locked -- --suite reviewed.json --corpus articles.json --output report.json --require-schema-v3` refuses schema-v1/v2 suites without publishing a report. Without the flag, backward-compatible exploratory evaluation is unchanged.
- Reports record `complete_prose_coverage_required`; the nested evaluation also records `annotation_schema_version`. These are reproducibility/policy receipts, not factual-entailment or reviewer-authenticity attestations.
- Unit and executable tests cover parsing the bare flag, rejecting a repeated flag, successful strict-corpus schema-v3 evaluation and rejecting a downgraded schema-v2 suite containing unreviewed prose with no output artifact.

## Exact-head evidence

The initial implementation `861da046ba15c7b1bb5b47032f73b67fa3e9042c` failed the Rust workspace formatting gate ([run 41503549532](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41503549532)). Formatting was corrected in source commit **`9252f5f8039c73ee7ed7ef329676ca2543b5e70b`**.

- [Rust workspace run 41503863314](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41503863314): passed, including the explicit executable example-test gate.
- [Android Rust ABI run 41503863177](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41503863177): passed.
- [Platform shells run 41503862980](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41503862980): pending at documentation time.

## Limits and remaining work

The flag prevents silent downgrade from schema-v3 coverage to older partial annotation when operators enable it. It does not independently prove that annotations are correct, that each claim is an atomic factual proposition, that cited passages entail the claim, or that the corpus and human labels are authenticated. RAG-003 remains open pending representative independently reviewed Wikimedia datasets, authenticated provenance, factual-entailment qualification and real-device tests. Do not check off RAG-003 based on this scoped implementation.
