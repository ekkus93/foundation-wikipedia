# MOB-004 — when a bookmark was saved (2026-11-27)

Code: `b4bee827` (store), `9c2f387a` (Android), `933f7df9` (desktop). MOB-004 remains unchecked.

## What changed

- **Store:** the user-state schema goes from version 3 to 4. Version 4 adds `bookmarks.saved_at` (Unix seconds, UTC, NULL or positive). The migration runs in the same transaction as the version bump, is a no-op when re-run, and a newer version (5) is refused and left untouched. Existing bookmarks keep NULL: their time was never recorded, so none is invented. New bookmarks record the time; re-bookmarking keeps the original; removing a bookmark removes its time. The privacy-generation checks on every write are unchanged.
- **Android:** Settings › History & Saved Content shows "Saved Nov 26, 2026" under a bookmark (`DateUtils`, device locale and time zone).
- **Desktop:** the bookmark row ends " · Saved 26 Nov 2026" (`toLocaleDateString`).
- **Old bookmarks:** both apps show no date.

Seen on the API 35 emulator: Tide, bookmarked by the test suite, listed as "Saved Nov 26, 2026".

## Tests

Store: migration from a hand-written v3 database (rows survive with no time; re-open is a no-op; a re-saved old bookmark stays timeless); a failed migration leaves v3; version 5 refused; time recorded, kept, cleared; non-positive times refused; stale generation writes nothing; a corrupt stored time is refused on open. FFI: new bookmarks carry a recent time, history entries none. Android and desktop unit tests for the date text; desktop IPC parser.

## Not done

- The desktop date line was not looked at on screen.
- Physical-device checks for MOB-004 as a whole.

## CI

CI on `65a3d685` (all of this work plus the rename): all nine workflows passed, incl. [Rust workspace 45961745860](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745860), [Android FFI bridge 45961745759](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745759), [Desktop launch smoke 45961745887](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745887), [Platform shells 45961745775](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745775) (`OK (48 tests)`).
