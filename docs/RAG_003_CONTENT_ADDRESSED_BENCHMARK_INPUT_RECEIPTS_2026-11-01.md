# RAG-003 — Content-addressed retrieval benchmark input receipts

**Date:** 2026-11-01 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `79bafc9b395a18057b21c2d7275f5f4f9a8c26e3`  
**Status:** Partial. RAG-003 remains unchecked.

## Problem

The retrieval benchmark report previously recorded a caller-asserted Git SHA, suite ID, threshold policy and ranking metrics, but not hashes of the article objects and labels actually evaluated. Two different input corpora or label files could therefore produce similarly identified reports, making historical comparisons and independent reproduction ambiguous.

## Implementation

The `wiki-search --example retrieval_suite_runner` CLI now emits a schema-v3 JSON report. It preserves the score fields and adds `input_receipts` with:

- `canonical_articles_json_sha256`: SHA-256 of a deterministic JSON serialization of the canonical `Article` objects that were indexed, ordered by project, page ID and revision ID. The serialization streams directly into a SHA-256 writer to avoid an extra corpus-sized allocation.
- `label_suite_bytes_sha256`: SHA-256 of the exact labeled-suite input bytes read for the run; even a whitespace-only label-file change changes this receipt.
- `canonical_article_count` and `corpus_source_kind` (`json_article_array`, `pack_articles_object`, or `local_snapshot_frames`).
- `source_authentication: caller_supplied_unverified` to prevent a content digest from being mistaken for Wikimedia publisher authentication.

The library's `run_retrieval_suite_json` schema-v2 output is unchanged; the schema-v3 extension applies only to the CLI report artifact. Existing `passed`, `retrieval`, `thresholds`, `suite_id`, and `claimed_source_commit_sha` fields remain present.

These hashes identify the decoded canonical articles and the label-file bytes. They are **not** a checksum of the original pack archive, raw dump files, SQLite catalog, or the publisher's release manifest. Corpus order is normalized for hashing; label-suite byte order is not.

## Regression tests and CI

Added a deterministic receipt regression verifying identical inputs produce identical output, changed suite bytes produce a changed digest, the canonical JSON empty-array digest is correct, and the report explicitly disclaims authentication. Extended the snapshot-backed retrieval runner regression to assert the schema-v3 report and its corpus-kind/count receipts.

- Initial implementation `fd0fde6a1b06176ac80e90547221634331a859b6` failed because `Cargo.lock` did not yet record the new direct `sha2` dependency ([Rust workspace 41380384481](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41380384481)).
- Corrected source and lockfile SHA **`79bafc9b395a18057b21c2d7275f5f4f9a8c26e3`** passed [Rust workspace 41380447189](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41380447189), [Platform shells 41380447151](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41380447151), and [Android Rust ABI qualification 41380447270](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41380447270).
- [Android record codec probe 41380447155](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41380447155) was still running at the time of this documentation.

## Remaining acceptance

RAG-003 still needs independently labeled representative questions, authenticated source-generation and label provenance, factual-answer entailment evaluation, and real-device retrieval measurements. Content-addressed receipts improve reproducibility but do not establish ground truth. The desktop chat interface still needs canonical evidence loading and explicit consent-gated inference wiring.
