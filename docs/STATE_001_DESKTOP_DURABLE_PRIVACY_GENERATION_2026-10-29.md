# STATE-001 — Desktop durable privacy-generation integration (2026-10-29)

**Scope:** Foundation Wikipedia mutable user state and Tauri desktop adapter; partial qualification only. Canonical tracking remains [WIKIPEDIA_AI_READER_TODO.md](WIKIPEDIA_AI_READER_TODO.md).

## Implemented

- `UserState::clear_personal_reading_data_for_generation` uses a SQLite `BEGIN IMMEDIATE` transaction to compare the caller's captured persistent generation, increment it, and delete bookmarks, positions, visit history and chat threads atomically. A stale full-reset request returns `false` and leaves newer personal data untouched. Pack owners, builder jobs and ordinary settings remain separate.
- Tauri captures the persistent generation when each relevant IPC command begins, before queuing its blocking worker. The existing process-local personal-data gate still orders same-process operations.
- Desktop bookmark toggle uses generation-guarded insert/delete; remove uses the generation-guarded delete. Visit recording uses `record_visit_for_generation`. History-only clear uses `clear_history_for_generation`; full clear uses the new guarded reset. Stale operations report errors instead of silently returning success.
- The bookmark toggle validates the generation again after its read/modify/write sequence; a cross-process reset between lookup and mutation cannot silently delete a fresh bookmark.
- Added two-handle disk-backed regression coverage for stale full reset, preservation of newer bookmark and pack ownership, and a desktop-native stale bookmark toggle/removal after a second connection resets state.

## Exact source and CI

- Initial implementation `a3950a3de62682ddd662db8bb99fe578d619af04` failed Rust formatting ([run 40975407586](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40975407586)).
- Formatting repair `0f2cf8c4fd14f642e6c2cc5c63a013e3011f7bf4` passed formatting but failed one newly added test due to an incorrect assertion about an already-stale maximum generation. A maximum value is a no-op when it does not equal the stored generation; overflow is relevant only after an exact match.
- Corrected source `ec08e162040e61dccbf6cc0e1c18f6c191d45741` passed [Rust workspace](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40975751423) on **attempt 2** (attempt 1 encountered an intermittent existing parallel-initializer SQLite `DatabaseBusy` test failure), [Platform shells](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40975751555) and [Android Rust ABI qualification](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40975751422). [Desktop launch smoke](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40975407697) passed for the original desktop implementation source, which is byte-for-byte unchanged in the corrected SHA. Android record codec probe `40975751471` was pending at this observation and is not privacy evidence.

## Remaining limitations

- Capturing SQLite generation at IPC entry currently opens the private state database synchronously in an async command; measure and move to a nonblocking, correctly ordered approach without losing the capture-before-queue invariant.
- Desktop history consent opt-out/re-enable still relies on a process-local visit epoch; durable cross-process consent-revocation fencing is not yet implemented.
- Personal-data reads, UI state and in-memory article cache are not yet coordinated across independent app processes; an external reset does not automatically notify a second process to clear its in-memory view.
- Legacy unguarded `UserState` methods remain callable by other adapters. Android/UniFFI integration, real application restart, privacy backups/restore lifecycle, physical-device tests and complete offline-media acceptance are not qualified.

**Disposition:** `STATE-001` remains unchecked. No branches or pull requests were created.
