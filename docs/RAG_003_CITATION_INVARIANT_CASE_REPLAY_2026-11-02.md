# RAG-003 — Citation-invariant reviewed-case replay guard (2026-11-02)

## Problem

The answer-support evaluator previously deduplicated cases by project, page ID, revision ID, Unicode-whitespace-normalized question and **raw answer**. Because the raw answer contains `[[cite:BLOCK_ID]]` metadata, a second case with the same visible prose and question could bypass duplicate detection by changing only citation block IDs and their corresponding review labels. The repeated factual answer would then contribute a second set of aggregate support/precision counts.

## Implementation

In `crates/wiki-search/src/answer_support.rs`, parse and validate the answer's citation markers **before** computing the duplicate-case identity. Use the marker-stripped visible prose, not the raw answer containing block IDs, for the existing Unicode-whitespace-normalized comparison. Retain the exact raw answer for SHA-256 receipts, UTF-8 claim spans, citation-marker association, per-claim verdicts and strict canonical corpus membership. This guard applies to schemas v1, v2 and v3.

A regression constructs a valid second case with a different case ID, different citation block IDs and matching updated citation annotations. The modified case passes alone under each schema but is rejected as a replay when paired with the original. Genuine different visible answers and different revisions remain independently eligible.

## Verification

Qualified implementation commit: `465cbefe00db1364ed54cc7961d7129c00c9b2aa`.

- Rust workspace (format, Clippy, unit tests and executable benchmark tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/41539728083 — passed.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41539728046 — passed.
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41539728114 — passed.

## Limits and next steps

This is an integrity guard for caller-supplied annotations, **not** semantic duplicate detection or factual-entailment grading. A paraphrased question or answer can still represent a near-duplicate. RAG-003 remains unchecked pending representative independently reviewed Wikimedia datasets, authenticated provenance, factual-entailment qualification, and real-device testing. The Android ABI workflow is a cross-build, not a physical-device benchmark.
