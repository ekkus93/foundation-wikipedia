# STATE-001 — History opt-out and stale-clear epoch isolation (2026-10-28)

## Scope

The Tauri desktop adapter now synchronizes `set_reader_history_enabled` with the same personal-data gate used for visit recording and privacy reset. Disabling history revokes already queued visit writes, even if the user later re-enables history. The existing SQLite `record_visit` preference check remains a second guard. Enabling history is generation-gated, so a delayed preference write from before a full personal-data reset cannot change the post-reset preference.

History-only clearing now acquires the gate's mutex and checks the captured personal-data generation **before** incrementing the visit generation. Previously a delayed history-clear command could increment the visit epoch after a full privacy reset even when the clear was rejected, invalidating unrelated new visits. Under the mutex, any earlier visit writer completes before deletion, while any old writer executing afterward observes a different visit generation.

This change does not clear history merely because the user disables future history recording. Existing history can still be removed with the explicit history-clear operation. Bookmarks remain independent.

## Implementation and verification

- Source: `desktop/src-tauri/src/lib.rs`.
- Initial source commit: `aa3d44251ca5974a2774254090b08f6ea69c1716`. Rust workspace passed, but native desktop test compilation failed because the new reset fixture was not mutable: [Desktop smoke 40709410194](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40709410194).
- Corrected source commit: **`242b7636498e95c4dc2faa78ba2a351db13f8910`**.
- [Rust workspace 40710769010](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40710769010): passed.
- [Platform shells 40710768974](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40710768974): passed.
- [Desktop launch smoke 40710768898](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40710768898): passed, including the native Tauri library test gate.

New native regressions:
1. `disabling_history_fences_queued_visits_even_after_reenable`: old visit generation rejected after disable/re-enable; a newly captured generation records a visit.
2. `stale_history_clear_cannot_invalidate_new_privacy_epoch`: a pre-reset clear or disable cannot modify the post-reset visit epoch or preference.

## Qualification limits

**STATE-001 remains unchecked.** This is in-process request ordering, not durable cross-process locking or an Android guarantee. It does not complete SQLite schema migrations, reading-position/chat/pack state, installed offline media, restart/device acceptance or the full product flow. The test fixtures simulate request ordering; browser-level concurrent IPC and actual device behavior remain to be qualified.
