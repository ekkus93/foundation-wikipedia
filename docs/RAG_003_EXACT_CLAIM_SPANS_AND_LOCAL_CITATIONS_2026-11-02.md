# RAG-003 — Exact reviewed claim spans and local citation association

**Date:** 2026-11-02 (America/Los_Angeles)  
**Passing implementation SHA:** `ff1feb369c69a60ce95bdd4413b2d0b5a3fe687b`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem

Schema-v1 reviewed claims identify a literal text fragment but not its occurrence in the answer. A benchmark could label a citation marker appearing elsewhere in the answer as support for a different claim, and could not distinguish two legitimate identical claims appearing in different places. Literal substrings also allowed partially overlapping annotations to inflate the number of reviewed assertions.

## Implemented contract

`wiki-search::answer_support::evaluate_answer_support_json` now accepts **annotation schema_version 2** as an opt-in extension. Each v2 claim requires `start_byte` and `end_byte`, the half-open UTF-8 byte interval of the exact literal `text` in the **original generated answer** (including citation-marker bytes). The validator rejects absent offsets, reversed/empty spans, non-character-boundary offsets, mismatched text, spans intersecting citation markers, and overlapping claims.

For each claim, the cited marker must occur after the claim's end and before the start of the next annotated claim. Every marker occurrence must belong to a cited claim in its own interval. This rejects a marker attached to a different claim while preserving repeated identical claim text at separate, non-overlapping positions. The marker and article-identity checks from v1 remain in force; strict `--corpus` membership can be combined with v2.

Existing schema-v1 inputs remain accepted and preserve their duplicate-literal-text restriction. Schema v1 cannot establish per-occurrence claim-to-marker attachment. The JSON score report retains its existing report schema version; the annotation-input schema version is independent.

## Verification

The implementation initially failed formatting at `e604fd2d4e0d5fec2bc088b829fb6c8414a00105`, then a Rust test-borrow error at `392806057d3ca3ce1448da2fe1e1c0451977f9b9`, then a historical unsupported-schema test expectation at `d67a3eb5c7ecf933ba9ebfdb934fe157294b052e`. All were corrected.

**Exact source SHA `ff1feb369c69a60ce95bdd4413b2d0b5a3fe687b` passed:**

- [Rust workspace — run 41466779641](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41466779641): formatting, strict Clippy, and workspace tests.
- [Platform shells — run 41466779387](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41466779387): passed.
- [Android Rust ABI — run 41466779803](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41466779803): passed.

New Rust regressions exercise a valid v2 suite, swapped claim citations despite both markers being present, missing offsets, overlapping claims, marker-only text, identical text at distinct offsets, v1/v2 incompatibility, and an invalid mid-UTF-8 span.

## Limitations and next steps

This enforces a conservative **annotation association convention**, not a semantic claim parser: a citation for an earlier claim that appears only after a later annotated claim will be rejected, and reviewers must annotate contiguous, non-overlapping literal spans. It does not discover unannotated claims, prove that a passage entails a claim, authenticate upstream Wikimedia article bytes, verify reviewer independence, or establish physical-device performance. Representative independently reviewed source-backed suites and factual-entailment qualification are still required. Do not mark RAG-003 complete.
