# STATE-001 — Visit-history deletion and privacy-safe read fencing

**Date:** 2026-10-28  
**Status:** Partial qualification; STATE-001 remains unchecked  
**Scope:** Desktop Tauri SQLite user-state IPC and process-local concurrency

## Failure modes

The earlier privacy reset gate protected queued bookmark and visit *writes*, but clearing only visit history did not invalidate a visit writer already queued before the deletion. That writer could subsequently recreate a visit the user had just erased. Bookmarks and history enumeration also executed without the personal-data gate, permitting reads queued before a privacy reset to surface old identities after deletion.

## Implementation

The desktop `DesktopPersonalDataGate` now tracks an independent visit generation alongside the personal-data reset generation. `record_reader_visit` captures both at IPC entry and checks them while holding the gate's mutex. `clear_reader_history` advances the visit generation before waiting on that mutex and executes the SQLite deletion inside the gate. A previously queued visit cannot reinsert cleared history, but a fresh visit can be recorded afterward. History-only deletion leaves existing bookmarks intact and does not invalidate unrelated bookmark operations.

`list_reader_bookmarks`, `list_reader_history`, and `reader_history_enabled` now also capture the personal-data generation at IPC entry and read under the same gate. Reads queued before a full privacy reset are rejected rather than returning obsolete private identities. A full reset invalidates both generations. Native writes already holding the lock finish before deletion.

## Verification

Implementation commit: `280e564c6333f6a9a833a7e0760d94671cef3ff4`.

The native Tauri library tests exercise the visit-history clear/queued-write race, bookmark retention, fresh post-clear visits, and pre-reset reads rejected after deletion. The existing full-reset stale bookmark/visit test was extended to use the new visit-generation check.

Exact-commit GitHub Actions passed:

- [Rust workspace — 40699707829](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40699707829)
- [Platform shells — 40699707822](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40699707822)
- [Desktop launch smoke — 40699707806](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40699707806)

## Remaining acceptance

This is an in-process mutex/generation fence, not a durable cross-process synchronization mechanism. It does not establish all user-state schema migrations, restore/backup qualification, real desktop restart/device acceptance, installed-pack ownership, complete offline media, or Android state parity. **STATE-001 stays unchecked.**
