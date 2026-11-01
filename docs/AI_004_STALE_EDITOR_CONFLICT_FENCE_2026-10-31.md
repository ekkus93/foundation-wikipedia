# AI-004 — Reject stale desktop profile editor writes

**Date:** 2026-10-31  
**Qualified implementation SHA:** `af6c13519a31f76cb37c1d27ddcb4f822285ad04`  
**Status:** Partial AI-004; parent remains unchecked.

## Failure mode

The earlier native SQLite compare-and-swap prevented two simultaneous read/modify/write operations from overwriting one another. It did not prevent a desktop window from editing an old profile form and submitting it *after* another window had saved newer profiles: the native handler would read a fresh collection and apply the old form to that fresh state.

## Change

- The desktop supplies its previously loaded `expectedProfiles` collection with each explicit save, activate or delete request.
- The native Tauri boundary validates the expected collection and requires it to match the current persisted v2 collection or read-only legacy-v1 compatibility view. A mismatch fails closed before changing anything.
- The existing atomic SQLite setting CAS remains in force after this comparison, closing the separate race between the new precondition read and the write.
- A failed desktop mutation refreshes the saved-profile list when possible, without replacing the user's unsaved endpoint/model draft or triggering any network request.

## Evidence

- Exact source commit: `af6c13519a31f76cb37c1d27ddcb4f822285ad04`.
- [Rust workspace 41266933547](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266933547): passed (format, strict Clippy, Rust tests).
- [Desktop launch smoke 41266933469](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266933469): passed.
- [Platform shells 41266933567](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41266933567): in progress when this report was written; recheck exact SHA.
- A new disk-backed two-handle SQLite regression confirms that a stale editor cannot create, activate or remove profiles over a newer saved collection, and that a fresh snapshot permits a later change.

## Scope limits

This is an optimistic conflict check using the normalized collection, not a persistent monotonic revision token. An identical collection reached through an intervening edit (ABA) is treated as equivalent. It does not provide cross-window push notifications, an interactive merge UI, real local/LAN inference, cloud consent, OS secret storage or physical-device acceptance. No AI-004 parent checkbox is qualified by this slice.
