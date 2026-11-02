# RAG-003 — Nonlexical reviewed-claim score padding guard (2026-11-02)

## Scope and defect

The human-reviewed answer-support evaluator accepts reviewer-supplied literal claim spans and citation verdicts. Prior to this increment, a punctuation-only annotation (for example, `!` followed by a `[[cite:...]]` marker) could be labeled `supported`. Multiple such annotations could inflate the supported-claim numerator and denominator, potentially concealing an unsupported factual claim under an aggregate or per-case support threshold. Schema v3's visible-word coverage did not itself prohibit nonlexical claim annotations.

## Implementation

`crates/wiki-search/src/answer_support.rs` now requires every literal reviewed claim to contain at least one Unicode alphanumeric character or underscore. The existing exact-span, word-boundary, citation-association, schema-v3 coverage and corpus-membership checks remain in force. Punctuation remains permitted *inside* an otherwise lexical claim; punctuation-only text cannot count as a separate factual claim. The check applies to annotation schemas v1, v2 and v3.

## Regression evidence

The Rust test `nonlexical_claims_cannot_pad_reviewed_support_scores` rejects punctuation-only claim text in legacy schema v1 and a schema-v3 extra claim with a locally associated supported citation. It also confirms a Japanese lexical claim is accepted, preventing an ASCII-only regression. Existing answer-support and executable benchmark runner tests remain part of the exact-head Rust workflow.

- Source commit: `a0d1b7c869bc05a3146d5d966b513bc92ca26a39` (directly on `master`).
- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41513477018), including formatting, strict Clippy, workspace tests and executable benchmark tests.
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41513477020).
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41513477010).

## Qualification boundary and next work

This closes one score-padding avenue but does not prove that reviewers separated factual propositions correctly, that labels are independent, or that a cited block semantically entails the claim. Representative independently reviewed Wikimedia evaluation datasets, authenticated source provenance, factual-entailment qualification and real-device testing remain outstanding. **RAG-003 stays unchecked.**
