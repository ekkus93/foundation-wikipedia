# AI-005 — Desktop read-only private chat history IPC

**Date:** 2026-11-01  
**Scope:** Partial AI-005 / UX-002; parent acceptance remains unchecked.  
**Verified source SHA:** `f7e1ec3582397d9d1348460de3adffedaafe14b2`

## Problem and scope

The shared SQLite store can atomically save citation-validated Q&A pairs and read bounded complete exchanges, but desktop code had no IPC entry point for listing an exact article revision's saved threads or retrieving their latest complete turns. This change adds **read-only** Tauri adapters without fabricating a chat generation UI or treating an online HTML preview as canonical evidence.

## Implementation

- `list_reader_chat_threads(project, page_id, revision_id)` returns up to 50 saved thread IDs for the exact page/revision; no other page or revision is queried.
- `read_reader_chat_exchanges(project, page_id, revision_id, thread_id)` returns at most 20 complete Q&A pairs with original row IDs and plain-text bodies; orphan or malformed rows are rejected by the shared `recent_chat_exchanges` reader.
- Both commands capture the durable privacy generation before queuing a blocking worker and check that generation before and after the SQLite read. A cross-process personal-data reset invalidates the result rather than returning stale private history.
- Commands use the existing app-owned private SQLite path and do not initiate network requests, inference, model discovery, or passage transmission.
- Persisted assistant text is **not** re-certified as a valid citation answer by this read; future UI must render it as plain text unless it separately establishes exact-revision citation provenance.

## Verification

A native desktop regression creates a revision-bound thread, saves a complete Q&A exchange, confirms thread listing and ordered returned message IDs/text, verifies wrong-revision isolation, then clears private data and confirms that the old generation can no longer read either threads or messages.

Exact source SHA `f7e1ec3582397d9d1348460de3adffedaafe14b2` passed:

- [Rust workspace — 41313889189](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313889189)
- [Platform shells — 41313889058](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313889058)
- [Desktop launch smoke — 41313889026](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41313889026)

## Remaining qualification

The React desktop chat surface does not call these commands; there is no complete chat flow, citation link handling, explicit per-request passage-sharing consent UI, canonical revision evidence loading, or live provider acceptance. The adapter exposes numeric SQLite IDs to the desktop IPC; frontend parsing and integer-precision policy should be reviewed before user-facing integration. **AI-002, AI-004, AI-005 and UX-002 remain unchecked.**
