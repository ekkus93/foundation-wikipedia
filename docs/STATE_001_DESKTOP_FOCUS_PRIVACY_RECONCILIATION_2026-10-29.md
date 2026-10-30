# STATE-001 — Focus-time desktop cross-process privacy reconciliation (2026-10-29)

**Scope:** Tauri cached article previews and React reader/settings session after another process advances the durable SQLite privacy generation.

## Implementation

`sync_reader_privacy_generation` reads the private SQLite generation on a blocking worker, returns a canonical decimal string (not an unsafe JavaScript `number`), and remembers the last generation observed by the native preview cache. On a change it bumps the cache epoch and clears cached article previews. The existing cache generation guard prevents old queued HTTP requests from repopulating the cleared cache.

On initial mount, window focus, and visibility restoration, React calls the sync command. The first successful value establishes a baseline. Subsequent changes invalidate the personal-data session and clear mounted article previews, tab/scroll snapshots, pending saved-bookmark navigation, and cached bookmark/history display. The History & Saved Content settings loader is session-fenced so a pre-reset IPC completion cannot restore stale identities; a new reader session triggers a fresh settings load.

`ReaderPrivacyGenerationMonitor` validates canonical nonnegative decimal strings up to SQLite's signed 64-bit maximum; it refuses unsafe numeric coercion and invalid inputs without changing its baseline.

## Regression and limits

Native Rust test covers idempotent cache reconciliation and generation invalidation. Node tests cover first observation, stable focus checks, reset changes, maximum supported generation and invalid values. Exact source SHA **`feb215b47a27b94a6f68ca100081925ac7596cd6`** passed [Rust workspace 40982290624](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40982290624), [Platform shells 40982290611](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40982290611), and [Desktop launch smoke 40982290530](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40982290530).

This is eventual focus-time reconciliation, **not** immediate cross-process notification while both windows stay foregrounded. A reset after a completed check remains possible until the next focus/visibility check; other app processes can still retain UI state temporarily. No physical desktop or Android device acceptance is claimed.

**Disposition:** `STATE-001` remains unchecked; direct `master` commit only.
