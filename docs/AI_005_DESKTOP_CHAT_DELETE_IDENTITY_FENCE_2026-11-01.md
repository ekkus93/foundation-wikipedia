# AI-005 — Desktop chat deletion identity fence

**Date:** 2026-11-01  
**Source commit:** `c55bd6d212e5476e97b0b8df2952214020d6bcdc`  
**Status:** Scoped frontend race correction; AI-005 remains open.

## Defect and implementation

The read-only conversation panel awaited a native SQLite delete and then unconditionally applied its success, failure and cleanup to React state. If the user navigated to a different article while the request was pending, a late response could overwrite that article's conversation list, clear its messages, show a stale notice or leave controls in the wrong state. Matching numeric thread IDs are not sufficient because IDs are shared across articles and may be reused in the UI.

`ReaderChatDeleteFence` binds each delete attempt to the exact Wikimedia project, page ID, revision ID, thread ID and a monotonic token. The component invalidates tokens when article identity changes or the panel unmounts, resets its deletion-busy state for the new article, and checks the current render's article identity before applying any completion. A new request cannot be confused with an earlier same-ID attempt. Native SQLite deletion still uses its own exact article/revision and privacy-generation guards.

## Qualification

`desktop/scripts/readerChatHistory.test.mjs` covers valid current requests, wrong thread, changed revision, changed project, explicit invalidation, and same-identity ABA after a new attempt. Exact source SHA passed:

- [Rust workspace — 41354131201](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41354131201)
- [Platform shells — 41354131271](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41354131271)
- [Desktop launch smoke — 41354131042](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41354131042)

## Limits and next steps

The guard **does not cancel an already dispatched native deletion**. It prevents an obsolete response from modifying a different frontend article state. This is deterministic unit/CI qualification, not an interactive multi-window race test. The desktop chat panel remains read-only because its online HTML preview is not a canonical article evidence source. The next priority is a verified canonical article loader and a consent-gated desktop inference adapter, then citation navigation, saved-answer persistence and Android acceptance. **AI-005/RAG-002 remain unchecked.**
