# AI-005 — Desktop saved-chat transcript read identity fence

**Date:** 2026-11-01  
**Verified source commit:** `64af9cf316b3d9074279e6c1e1ed65f743fef076`  
**Status:** Scoped AI-005 frontend correction; AI-005 remains unchecked.

## Problem

The desktop read-only chat panel previously used an effect-local `active` flag to ignore obsolete `read_reader_chat_exchanges` responses. A user could select another saved thread and a previous IPC response could settle before the old effect's cleanup ran, briefly applying the previous thread's transcript or error/loading state to the newly selected conversation. A previous transcript could also remain visible during the render before the new effect cleared it.

## Correction

`ReaderChatExchangeReadFence` uses an independent monotonic token bound to the exact project, page, revision and thread ID. Every success, failure and completion checks the current article and selected thread synchronously, not only effect cleanup. Thread changes, deletion, article changes and unmount invalidate the old read. A `loadedThread` presentation guard ensures exchanges render only for the thread that produced them; switching the selection clears the guard before an asynchronous read starts. Existing deletion fencing and native SQLite privacy-generation checks remain intact.

## Automated evidence

`desktop/scripts/readerChatHistory.test.mjs` now covers wrong-thread and wrong-revision read completion, newer-request supersession, invalidation and repeated-thread ABA. The existing deletion-fence regressions continue to pass. Exact source SHA passed:

- [Rust workspace — 41359622334](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41359622334)
- [Platform shells — 41359622357](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41359622357)
- [Desktop launch smoke — 41359622369](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41359622369)

## Limits and next steps

This protects frontend state from late transcript reads; it does not cancel a dispatched SQLite read or prove interactive multi-window behavior. The panel remains read-only. Loading canonical article evidence, previewing exact passages and selected destination, obtaining explicit inference consent, persisting newly validated answers, and navigating citations remain required. Live local-model and Android acceptance remain open. **AI-005/RAG-002 remain unchecked.**
