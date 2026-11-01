# AI-005 — Explicit desktop saved conversation deletion

**Date:** 2026-11-01  
**Scope:** Partial AI-005 / UX-002; not parent completion.  
**Verified implementation SHA:** `f79d3a466668cc2b5995cae09a0151c319701251`

## Implementation

The desktop's read-only saved article conversations panel now offers **Delete this conversation…** followed by a separate **Yes, delete conversation** confirmation naming the selected thread. Switching conversations cancels pending confirmation. During a deletion, the selector and confirmation controls are disabled; on success the removed thread disappears from the list, the next available thread is selected, and the panel reports that local private storage changed. Failure does not falsely claim deletion.

The native Tauri `delete_reader_chat_thread` command runs on a blocking worker, captures the durable personal-data generation before queuing, serializes through `DesktopPersonalDataGate`, and calls the existing `UserState::delete_chat_thread_for_generation`. The SQLite DELETE includes the exact project/page/revision/thread identity and the captured persistent privacy generation; it cascades only the deleted thread's messages. A concurrent personal-data reset cannot authorize a stale deletion against a newly reused SQLite row ID. Other article threads, bookmarks, packs and ordinary settings are untouched.

## Automated verification

Native regression covers wrong article/revision no-op, successful targeted deletion and message cascade, idempotent repeat, preservation of an independent thread, and stale generation refusal after personal-data reset. The desktop frontend build/typecheck and smoke launch passed.

Exact implementation SHA `f79d3a466668cc2b5995cae09a0151c319701251` passed:

- [Rust workspace — 41318922103](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41318922103)
- [Platform shells — 41318922078](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41318922078)
- [Desktop launch smoke — 41318922134](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41318922134)

## Remaining

This does not implement live AI conversation generation, article-grounded consent, verified citation links, real Ollama/llama-server LAN acceptance, Android chat, or interactive multi-process acceptance. No manual confirmation click was performed. **AI-002, AI-004 and AI-005 remain unchecked.**
