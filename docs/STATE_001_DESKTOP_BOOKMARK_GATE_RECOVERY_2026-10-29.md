# STATE-001 — Desktop bookmark gate recovery after privacy resets (2026-10-29)

## Scope and defect

The desktop Bookmark FAB serialized toggles using a single boolean in-flight flag. Local personal-data deletion and external SQLite privacy-generation reconciliation invalidated stale completion messages, but did not release that flag. If the old IPC stalled, new bookmarks remained blocked after the reset. Unconditionally releasing the flag from a late old `.finally` could also unlock a *newer* request.

## Implementation

- Added `ReaderBookmarkInFlight`, with monotonically distinct request tokens, one active operation and explicit invalidation.
- On external generation reconciliation and at the start of local personal-data deletion, invalidate the old token immediately, without waiting for native IPC to settle.
- Each Bookmark FAB invocation captures its own token and releases the gate only if that token is still active. The existing `ReaderPersonalDataSession` revision guard independently suppresses stale success/error UI messages.
- Added two deterministic Node regressions for unresolved old IPC, multiple reset cycles, stale failed requests, old finalizers, and fresh-operation recovery. Registered the tests in `desktop/package.json`.

## Exact-head evidence

Implementation commit: `b6c41b37657f7d5b6097b0ad61bd5e40c12a8341`.

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40994660289 — passed.
- Platform shells (frontend typecheck, Node tests, bundle): https://github.com/ekkus93/foundation-wikipedia/actions/runs/40994660258 — passed.
- Desktop launch smoke: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40994660264 — passed.

## Remaining qualification

This is deterministic request-gate testing, not a real-browser or physical-desktop race exercise. It does not cancel a native SQLite/HTTP operation, provide immediate cross-process push notification, wire Android user-state adapters, or qualify backup/restore/device acceptance. Keep **STATE-001 unchecked** pending full end-to-end criteria.
