# STATE-001 — Ordered desktop appearance persistence (2026-10-29)

## Failure mode

The desktop theme selector applied every choice immediately to React but fired each `set_reader_theme` Tauri call independently. With concurrent native workers, an older SQLite write could finish after a newer user choice and overwrite it on disk. An older failed write could also display a stale error despite a newer choice succeeding.

## Implementation

- `desktop/src/readerLatestPreferenceWriter.ts` serializes asynchronous preference writes through an ordered promise chain. Each invocation has an identity; only the newest invocation is allowed to display a write failure. A failed write does not prevent later choices from being persisted.
- `desktop/src/App.tsx` routes theme persistence through this writer without delaying optimistic React theme changes or altering the existing startup-read revision guard.
- `desktop/scripts/readerLatestPreferenceWriter.test.mjs` verifies an older blocked write cannot overlap a newer write, a superseded failure cannot replace the new choice, and the latest write failure is correctly reported with later recovery.
- `desktop/package.json` registers the tests in the ordinary frontend validation command.

## Exact-head CI evidence

Source commit: `38e36e24d8b00d1b5a5592000871735edf3919f5` on `master`.

- [Rust workspace](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40995461805): passed.
- [Platform shells](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40995462031): passed, including frontend typecheck/build/Node tests, Android shell, and native Tauri adapter.
- [Desktop launch smoke](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40995461726): passed.

## Remaining acceptance

The tests exercise the real queue implementation with deterministic simulated asynchronous operations, not a mounted React browser or physical desktop. This orders writes **within one process**; it does not solve simultaneous choices from independent desktop processes or cross-process settings refresh. It does not establish all of STATE-001, STATE-002 or STATE-003. Keep those roadmap tasks unchecked pending end-to-end acceptance and other missing functionality.
