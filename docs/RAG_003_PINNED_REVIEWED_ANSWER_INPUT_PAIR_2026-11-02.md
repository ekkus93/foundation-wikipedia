# RAG-003 — Operator-pinned reviewed-answer benchmark inputs

**Date:** 2026-11-02 (America/Los_Angeles)  
**Exact passing source SHA:** `ee39b4e83eef77b91d716bea315219067129324d`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem and scope

The strict human-reviewed answer-support benchmark already checked canonical block membership and reported the SHA-256 of the supplied annotation suite and corpus. However, it did not **enforce** a previously selected pair of inputs. A silently changed corpus or annotation file could therefore produce a superficially comparable benchmark report. Retrieval evaluation already offered an optional operator-pinned input pair; the reviewed-answer path now has the equivalent safeguard.

## Implementation

- Added optional `--expect-suite-sha256` and `--expect-corpus-sha256` to `crates/wiki-search/examples/answer_support_runner.rs`. Both must be supplied together, with `--corpus`; each must be exactly 64 lowercase hexadecimal characters.
- Read each input from its existing single bounded file handle. Compare the supplied pin to the SHA-256 of the **exact raw file bytes** consumed, not a reserialized or normalized representation. Mismatch rejects before scoring or writing a report.
- Successful reports now include `operator_input_pins_verified`, which is true only when both pins were supplied and matched. `evidence_membership_checked` remains a separate receipt. Unpinned exploratory mode is retained for compatibility.
- Regression tests exercise valid paired pins, missing/unpaired/malformed pins, an altered reviewed suite, and a corpus hash mismatch; failures leave no report behind. The existing per-case and aggregate quality gates remain unchanged.

Example (hashes are operator assertions, **not** publisher authentication):

```bash
sha256sum reviewed_answers.json canonical_articles.json
cargo run -p wiki-search --example answer_support_runner --locked -- \
  --suite reviewed_answers.json \
  --corpus canonical_articles.json \
  --output reviewed_report.json \
  --expect-suite-sha256 '<64-lowercase-hex-suite-sha256>' \
  --expect-corpus-sha256 '<64-lowercase-hex-corpus-sha256>' \
  --min-case-claim-support-per-mille 800 \
  --min-case-citation-precision-per-mille 800
```

## Exact-head CI

- [Rust workspace 41440065077](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41440065077): passed, including formatting, strict Clippy and tests.
- [Platform shells 41440065029](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41440065029): passed.
- [Android Rust ABI qualification 41440065071](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41440065071): passed.

Intermediate source SHAs `842ed6efa72e8ac580107cefc51ed1488840139a` and `8be1ccb3e4022bd383832ac6f37946bb03f087e3` failed formatting; the corrected SHA above passed.

## Unqualified requirements

The input pins prevent accidental or adversarial input substitution **relative to the operator's chosen hashes**. They do not prove that the corpus came from a verified Wikimedia publication, that human labels were independently reviewed, that a model saw cited passages, or that the answer is factually entailed by them. Representative independently labeled Wikimedia cases, provenance authentication, end-to-end chat evaluation and real desktop/Android device qualification are still required. Do not check off RAG-003.
