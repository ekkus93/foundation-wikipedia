# AI-005 — Desktop read-only exact-revision chat replay

**Date:** 2026-11-01  
**Scope:** Partial AI-005 / UX-002, not completion.  
**Verified source SHA:** `e04eafb84fca768a371baf9dfd5eccfd3de20ade`

## Implementation

The desktop Chat speed-dial action opens a read-only, scrollable history panel for the active article tab's exact project, page ID and revision ID. It calls the existing privacy-generation-fenced Tauri `list_reader_chat_threads` and `read_reader_chat_exchanges` commands, then displays a bounded selection of saved conversations and up to 20 complete exchanges.

`desktop/src/readerChatHistory.ts` independently validates untrusted IPC arrays: at most 50 unique positive safe-integer thread IDs, at most 20 complete question/answer pairs with increasing positive safe-integer SQLite row IDs, nonblank UTF-8 bounded message bodies (64 KiB each), and well-formed UTF-16. It rejects sparse, malformed and duplicate identities. An SQLite `i64` outside JavaScript's exact integer range fails closed rather than routing to a rounded conversation ID.

`desktop/src/ReaderChatHistory.tsx` renders saved question and answer bodies as React text nodes, not HTML, Markdown or active citation links. A visible warning explains that historical model output is not revalidated against article evidence. An online text-only preview is **not** a canonical revision evidence map and cannot be used to authorize a new model request.

Switching the active tab or article revision keys the panel to the new identity. Pending IPC results are discarded on unmount/identity change; local or cross-process privacy resets close the panel and purge prior previews. Closing the panel does not delete persisted chat.

## Verification

The Node test `desktop/scripts/readerChatHistory.test.mjs` covers positive ordered exchanges, malformed/unsafe IDs, duplicate threads, sparse arrays, incomplete/reversed turns, oversized text, invalid surrogate sequences, multilingual text and adversarial HTML-looking answer strings. The existing frontend test command includes this regression.

Exact source SHA `e04eafb84fca768a371baf9dfd5eccfd3de20ade` passed:

- [Rust workspace — 41317964323](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41317964323)
- [Platform shells — 41317963904](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41317963904)
- [Desktop launch smoke — 41317963723](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41317963723)

## Remaining

The panel is intentionally **read-only**. It does not invoke the consent-gated Rust RAG method, load canonical revision-scoped evidence, stream generation, persist newly validated answers, verify historical citation links or implement a full movable/resizable chat overlay. Live Ollama/llama-server LAN acceptance, cloud credentials/consent, interactive desktop acceptance and Android chat remain open. **AI-002, AI-004 and AI-005 stay unchecked.**
