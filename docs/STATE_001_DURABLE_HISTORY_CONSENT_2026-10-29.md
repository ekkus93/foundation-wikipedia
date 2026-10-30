# STATE-001 — Durable cross-process history consent (2026-10-29)

**Scope:** Shared SQLite user-state API and production desktop Tauri history-preference command. This follows [desktop durable privacy generation integration](STATE_001_DESKTOP_DURABLE_PRIVACY_GENERATION_2026-10-29.md) and is partial qualification.

## Privacy invariant and implementation

A visit initiated while optional history was enabled must not be inserted after another process opts out and later opts in again. Merely checking `history.enabled=true` at insertion time would permit an old queued visit to reappear after consent is re-enabled.

`UserState::set_history_enabled_for_generation(generation, enabled)` now opens an SQLite IMMEDIATE transaction and compares the request's captured durable privacy generation. An outdated request returns `false` without changing consent. Disabling history advances the generation and saves `history.enabled=false` atomically; enabling history saves `true` without advancing the generation. The existing `record_visit_for_generation` SQL predicate therefore rejects pre-opt-out visits even if a second process later enables history. The same conservative generation invalidates other old personal writes; those callers must recapture a token.

The Tauri `set_reader_history_enabled` IPC command captures the persistent generation at entry, then executes the guarded preference update inside the existing process-local personal-data gate. Disabling consent still revokes queued visits in that process. Stale preference requests report an error rather than silently overriding newer consent.

## Synthetic verification

The disk-backed regression opens two independent `UserState` connections and exercises initial opt-in, accepted visit, opt-out, rejection of an old visit, rejection of a delayed opt-in, fresh opt-in and visit, and rejection of a delayed opt-out. It verifies two accepted visits and that the newer consent choice remains enabled.

- Initial code `f7ba404f10537918fbfb109f26cd7abdd73c14a3` failed only `cargo fmt --all -- --check` ([Rust workspace 40976489911](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40976489911)).
- Corrected exact source **`228c607bf76d81efa35ce9f56d9c0f1edc994462`** passed [Rust workspace 40976573963](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40976573963), [Platform shells 40976573888](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40976573888), and [Android Rust ABI 40976573961](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40976573961).
- [Desktop launch smoke 40976489859](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40976489859) passed against the original desktop implementation, unchanged in the formatting-only follow-up.
- [Android record codec probe 40976573877](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40976573877) failed on attempt 1 before emulator startup due to an invalid Android SDK emulator ZIP; a retry was accepted and attempt 2 was in progress at the time of writing. It is not evidence of desktop history-consent behavior.

## Unqualified behavior

The synchronous SQLite generation read at desktop IPC entry still needs a nonblocking performance evaluation. Independent processes do not automatically invalidate each other's in-memory article/Settings UI state or online HTML cache. Legacy unguarded shared APIs remain available for adapters that have not migrated. Android/UniFFI integration, physical-device acceptance, complete privacy backup/restore behavior and offline pack fidelity remain outstanding.

**Disposition:** `STATE-001` remains unchecked. Work committed directly to `master`, without a branch or PR.
