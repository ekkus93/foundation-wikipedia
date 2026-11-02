# RAG-003 — Per-case human-reviewed answer-support quality gates

**Date:** 2026-11-02 (America/Los_Angeles)  
**Exact passing source SHA:** `945f993a4fa10d5dbb3ddd295a2e2e97bc572d8b`  
**Status:** Partial; RAG-003 remains unchecked.

## Failure mode

The existing answer-support runner computed aggregate claim support and citation precision. A large number of strong reviewed answers could mask an individual answer with no supported claims or citations, even if aggregate score thresholds passed. Retrieval benchmarking already has optional per-question floors; answer-support evaluation needed the analogous gate.

## Implementation

- Added `--min-case-claim-support-per-mille` and `--min-case-citation-precision-per-mille`, each accepting an integer from 0 to 1000. The default 0 disables the corresponding per-case gate for backward compatibility.
- For an enabled gate, every case is checked against its own reviewed numerator/denominator. An empty reviewed denominator fails closed instead of being treated as perfect support or precision. All cases are included, so a weak answer cannot be masked by a strong one.
- Reports retain existing aggregate gates and add deterministic `gate.case_claim_support_gate` and `gate.case_citation_precision_gate` receipts: enabled flag, threshold, evaluated case count, sorted failed case IDs, and pass/fail.
- Failed quality gates still publish a no-clobber diagnostic JSON report and return a failing CLI exit code. Invalid annotations or missing canonical evidence still reject before report publication.
- Added unit tests for masked weak cases, disabled gates, empty denominators and invalid/duplicate flags. An executable-path regression with an exact canonical corpus verifies that a 50%-passing aggregate cannot override a failed 80%-per-case floor and that both failed case IDs appear in the saved report.

Example:

```bash
cargo run -p wiki-search --example answer_support_runner --locked -- \
  --suite reviewed_answers.json --corpus canonical_articles.json \
  --output reviewed_report.json \
  --min-claim-support-per-mille 500 \
  --min-citation-precision-per-mille 500 \
  --min-case-claim-support-per-mille 800 \
  --min-case-citation-precision-per-mille 800
```

## Exact-head verification

- [Rust workspace 41432746753](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41432746753): **passed** (format, strict Clippy and tests).
- [Platform shells 41432746643](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41432746643): **passed**.
- [Android Rust ABI 41432746666](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41432746666): **passed**.

Intermediate source commits `1b1697ade4ebc79e73f68883052f2a6ba1b14ad8` and `d2e32f975204fac5eeb8238fe2d054c4ac6e4aeb` failed formatting checks; corrected source above passed.

## Remaining

The corpus and human support labels are still caller supplied, not independently authenticated or representative of Wikimedia production articles. This evaluates human judgments; it does not automatically determine factual entailment or prove the model received the cited passages. Representative independently reviewed cases, pinned authoritative source provenance, end-to-end chat evidence and real desktop/Android qualification remain outstanding. Do not check off RAG-003.
