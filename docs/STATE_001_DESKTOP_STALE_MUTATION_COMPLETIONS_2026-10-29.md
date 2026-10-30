# STATE-001 — Desktop stale mutation completion qualification (2026-10-29)

## Scope and observed failure mode

A durable SQLite privacy-generation fence prevents old operations from mutating newer personal rows, but the desktop UI also receives asynchronous command completions. The previous Settings handlers for history-consent updates, history clearing and bookmark removal could still set local React state after a second process cleared data and the UI reconciled. A stale success could overwrite newly loaded history consent, remove newer bookmark/history rows from the view, or display a misleading success message. A stale failure could replace a fresh session's error, and an old `finally` could release a newer request's busy indicator.

## Implementation

- `desktop/src/App.tsx`: Each of the three personal-data mutation handlers captures a `ReaderPersonalDataSession` revision before invoking native IPC. They refuse to apply success, failure or finalization effects when the session was reset. They also refuse new operations during a local reset.
- External generation reconciliation releases stale saved/history busy flags, clears obsolete errors, invalidates old saved state and triggers the existing saved-state reload. Old completions cannot interfere with that reload.
- Successful local personal-data deletion marks history consent unknown until the saved-state reload; its `finally` releases any history busy indicator owned by a now-stale pre-reset operation.
- `desktop/scripts/readerPersonalDataSession.test.mjs` adds deterministic asynchronous completion tests for stale success and error/finally behavior.

## Exact-head evidence

- Source `c92aa4f4fcc0dee31e3294d51905d5d08b709b54`: [Rust workspace 40991122705](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40991122705), [Platform shells 40991122706](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40991122706) and [Desktop launch smoke 40991122716](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40991122716) all passed. Platform shells includes desktop typecheck/build/Node tests.
- Follow-up source `695debe4e14608addb90fac98508b948ce56f842`: [Rust workspace 40991597929](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40991597929), [Platform shells 40991597923](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40991597923), and [Desktop launch smoke 40991597952](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40991597952) were running when this note was prepared. Their final results require separate exact-head inspection.

## Remaining acceptance

The tests model deferred completions against the actual session utility, not a full mounted React component or real interprocess UI integration. Cross-process notification is bounded polling, not push. Native generation predicates and the UI session fence are complementary; neither establishes physical desktop/Android privacy acceptance, backup/restore lifecycle completeness, or all of STATE-001. Keep STATE-001 unchecked.
