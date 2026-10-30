# STATE-001 — Desktop cross-process personal read fencing (2026-10-29)

**Scope:** Desktop Tauri IPC bookmark list, history list and history-consent preference reads; partial qualification of [STATE-001](WIKIPEDIA_AI_READER_TODO.md).

## Privacy invariant

A request that begins before another Foundation Wikipedia process clears personal reading data or revokes history consent must not return its previously captured private rows as a successful response after that reset. The process-local `DesktopPersonalDataGate` only protects operations inside one process and cannot by itself detect another process.

The three read commands now capture `UserState::personal_generation()` at IPC entry, before their queued blocking worker. `read_personal_data_for_generation` compares that token to the SQLite generation both before and after the read. A mismatch is reported as an error; a fresh request captures a fresh token. This complements the already generation-guarded desktop writes.

## Regression coverage

- Two independent disk-backed SQLite handles simulate a reset during a bookmark read. The helper refuses the loaded pre-reset rows after the other handle commits the reset.
- Queued old-generation bookmark/history/preference reads are rejected, while fresh-generation reads return empty lists.
- A history opt-out advances the generation, rejecting a stale history-consent read while permitting a fresh read of disabled consent.

## Limits and acceptance

This is an optimistic two-check fence, not an OS-level cross-process notification or an atomic guarantee against a reset *after* the second check but before the UI consumes the response. Existing frontend session/cache state in another process still requires explicit invalidation or focus-time reconciliation. The IPC-entry SQLite open is synchronous and needs a latency evaluation. Android adapter and real desktop/Android device qualification remain outstanding.

**Exact source CI:** `15a16bac4d7836e06e4fb2ebb3d6ae89849f6a44` passed [Rust workspace 40981774732](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40981774732), [Platform shells 40981774740](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40981774740), and [Desktop launch smoke 40981774863](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40981774863).

**Disposition:** `STATE-001` remains unchecked; no branch or PR was created.
