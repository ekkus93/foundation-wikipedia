# STATE-001 — Desktop privacy reset: queued-write generation fence

**Date:** 2026-10-28  
**Scope:** desktop Tauri private user-state IPC and React bookmark UI  
**Status:** Partial qualification; STATE-001 remains unchecked

## Failure mode and correction

Deleting personal reading data already cleared SQLite rows, the online HTML cache and mounted reader sessions. However, the native IPC used independent blocking workers: a history visit or bookmark toggle queued before deletion could execute afterward and recreate cleared personal data. Similarly, an in-flight bookmark result could show a stale success message after a reset.

The desktop now manages a process-local `DesktopPersonalDataGate` with an atomic generation and exclusive write mutex. Bookmark toggles, bookmark removals and visit-history writes capture the generation at IPC entry. They recheck it **under the mutex** immediately before opening and writing the private SQLite database. A privacy reset increments the generation before waiting for the same mutex, so a write that started first finishes before deletion and a queued old-generation write cannot execute afterward. The reset continues to clear the in-memory online cache. Normal post-reset writes use the new generation.

The React `ReaderPersonalDataSession` independently blocks new bookmark actions while reset is pending and rejects callbacks from older bookmark actions, even when the native worker reports a late success or failure. This UI fence does not replace the native SQLite write gate.

## Evidence

- Native source: `acae5dd6efc7da7bc07a7da188c601f741140d01`. In-memory SQLite regression covers existing visit/bookmark removal, stale-generation write refusal and new-generation bookmark success. Exact-head CI: [Rust workspace 40690982297](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40690982297), [Platform shells 40690982334](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40690982334), [Desktop launch smoke 40690982386](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40690982386) — all passed.
- Desktop UI source: `b7c569600061637e1b3cb6ed24d4b108c330a953`. Node tests verify reset idempotence, blocked writes, stale callback rejection and post-reset recovery. Exact-head CI: [Rust workspace 40691674357](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40691674357), [Platform shells 40691674399](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40691674399), [Desktop launch smoke 40691674443](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40691674443) — all passed.

## Boundaries and remaining work

This is an **in-process** serialization/generation fence for selected desktop personal-data writes; it is not an interprocess lock, durable migration proof or protection against a separate uncoordinated writer. Already running HTTP requests are not forcibly canceled. A newly initiated legitimate visit after deletion may be recorded if the history preference remains enabled. Real desktop privacy-reset interaction, Android integration, installed-pack isolation, full schema/migration coverage and physical-device acceptance remain outstanding. Do not mark STATE-001 complete.
