# STATE-001 — Atomic SQLite read snapshot for saved-content panel (2026-10-29)

## Problem

The desktop Settings saved-content panel already used one Tauri IPC and a durable privacy-generation check, but its three native reads (bookmarks, consent and visits) were not in one SQLite read transaction. A second process could update these datasets without incrementing the privacy generation, leaving a panel response containing values from different committed database states.

## Change

- `wiki-store::user_state::UserState::saved_panel_snapshot_for_generation` now starts a deferred SQLite transaction, checks the expected generation on its first SELECT (pinning a WAL snapshot), and reads bounded, validated bookmark, consent and visit rows through that same connection.
- The transaction commits before a fresh generation check. A cross-process privacy reset committed while the old snapshot was pinned therefore rejects the response rather than returning stale identities.
- `desktop/src-tauri/src/lib.rs` maps the typed native tuple to the existing Tauri IPC response. No frontend contract or stored schema changes.
- The native regression uses two independent SQLite connections. One writes bookmark, history and consent updates after the reader pins its snapshot, demonstrating a coherent old tuple and fresh new tuple. A second mid-read mutation advances the privacy generation and demonstrates fail-closed rejection.

## Qualification

- Initial source `50d2a82b49c09d652b2eecdd7eb575ad725ae461` failed formatting only: [Rust workspace 40999078332](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40999078332).
- Corrected source **`fa99d0331b459d5de755eba925472e72c0bae71f`** passed [Rust workspace 40999140641](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40999140641) (format, strict Clippy and workspace tests), [Platform shells 40999140713](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40999140713) (frontend, Android shell, Tauri native checks), and [Android Rust ABI 40999140643](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40999140643).
- Android record codec probe `40999140603` was still in progress at documentation and must be rechecked; it is not evidence of real-device acceptance.

## Limitations

The read is a point-in-time database snapshot, not a guarantee that no process changes data immediately after the final generation check. The existing desktop privacy poll and session-revision fence still handle later external resets. No native push notification, real desktop browser automation, Android personal-data UI, backup/restore end-to-end, or physical-device test is claimed. The implementation assumes the existing private SQLite/WAL database and valid schema. **STATE-001 remains unchecked** until its full acceptance requirements are met.
