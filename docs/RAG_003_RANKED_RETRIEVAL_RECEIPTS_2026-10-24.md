# RAG-003 — Ranked Retrieval Evidence Receipts (2026-10-24)

## Implementation

The shared `wiki-search::evaluation` scorer now records each top-k retrieved passage in rank order with exact Wikimedia project/page ID, revision ID, block ID, lexical score and whether the caller-labeled gold set includes it. This makes a failed no-answer regression actionable: the report identifies the exact retrieved passage instead of providing only a false-positive count. Gold identities are still checked against the indexed revision before any scoring.

The retrieval report schema advances from **v1 to v2** because each case now includes `ranked_evidence`. The checked-in v1 fixture is retained as historical evidence; the v2 fixture is the byte-for-byte regression baseline. The JSON suite input schema remains v1 and existing `run_retrieval_suite_json` callers need no API changes.

## Qualification and limitations

Unit tests assert the ranked identity/score of an intentionally false-positive Physics passage, the absence of hits for an unrelated query, exact relevant evidence, and order-invariant JSON reports. The existing synthetic GitHub Actions gate now emits the richer v2 report.

These receipts are **not** factual entailment scores and do not authenticate the underlying corpus or human relevance labels. Representative official Wikimedia snapshots, independently reviewed labels, production-pack reports, answer-support grading and physical-device measurements remain required. **RAG-003 stays unchecked.**
