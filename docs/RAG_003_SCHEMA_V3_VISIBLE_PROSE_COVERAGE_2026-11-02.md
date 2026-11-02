# RAG-003 — Schema-v3 complete visible-prose coverage

**Date:** 2026-11-02 (America/Los_Angeles)  
**Passing implementation SHA:** `bd9ee1c564d75108776636f758f776ba2f366bfd`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem

Schema-v2 human-reviewed answer annotations bind literal claims to exact UTF-8 spans and their local citation markers, but they allow a reviewer to omit an entire factual sentence or phrase from the annotation list. That omitted text contributes neither a supported nor an unsupported claim to the denominator, so a superficially high score may conceal unreviewed assertions.

## Change

`wiki_search::answer_support::evaluate_answer_support_json` now accepts **annotation schema v3**. It retains v2's exact, nonoverlapping claim spans, Unicode word-boundary checks, and local citation association. In addition, every Unicode alphanumeric or underscore character in the original answer must lie inside a reviewed claim span or inside a parsed `[[cite:BLOCK_ID]]` marker. Punctuation and whitespace can remain between claims. Any unannotated visible word returns `UnreviewedProse(case_id)` before scoring or report publication.

Schema v1 and v2 are unchanged for compatibility; v3 is opt-in by setting `schema_version` to `3` in the reviewed suite. The existing `answer_support_runner` accepts the new version through the shared evaluator, including strict `--corpus` membership and optional input SHA-256 pins.

## Regression and CI

The Rust regression `answer_support::tests::v3_requires_complete_visible_word_coverage` verifies that a fully annotated answer passes, unreviewed English and accented Unicode prose fail, punctuation-only additions pass, and schema-v2 partial annotations remain accepted.

The first implementation `71251cef89fb4cc18ad039796b528de78445439f` failed one legacy unit test expecting schema 3 to be unsupported. That test now reserves schema 4 as unsupported.

Corrected implementation `bd9ee1c564d75108776636f758f776ba2f366bfd` passed:
- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41485513533
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41485513619
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41485513737

## Qualification limits

Complete **word-span** coverage is not complete **factual-claim** coverage: a reviewer may combine several propositions in one span or mislabel entailment. No independent Wikimedia source authentication, representative independently reviewed dataset, factual entailment qualification, or physical-device acceptance is established. Keep RAG-003 unchecked pending those requirements.
