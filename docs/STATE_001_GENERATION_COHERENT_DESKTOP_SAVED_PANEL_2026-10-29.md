# STATE-001 — Generation-coherent desktop saved-content reads (2026-10-29)

## Risk and scope

The Settings saved-content panel previously fetched bookmarks, optional history consent and visit history through three independent Tauri commands. Each native command independently validated the SQLite privacy generation, but a cross-process deletion or consent revocation between commands could still produce a mixed pre-/post-reset response. The panel could render saved identities from an old privacy epoch alongside new consent/history state until the next poll.

## Implementation

- `desktop/src-tauri/src/lib.rs` adds `saved_panel_rows` and `read_reader_saved_panel`, which capture a single persistent generation before starting the native worker, read all three datasets through one private SQLite handle under the native personal-data gate, and check the same generation both before and after all reads.
- `desktop/src/App.tsx` uses one IPC for the panel rather than three independent requests. The existing session-revision fence still rejects stale completions following a local or external reset.
- `desktop/src/readerResponse.ts` adds `parseReaderSavedPanel`, which validates all three members as a tuple before any component state is updated. `desktop/scripts/readerResponse.test.mjs` verifies valid/invalid tuples and nested row guards.
- A two-handle SQLite native test verifies that a previously captured generation is rejected after another connection deletes personal data, and that a fresh read contains no stale bookmark/history identities.

## Exact-head CI

Source commit: `87fc5bdf016a43d06d9ab59fb6ac6514ba424d87`.

- [Rust workspace](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40996494661): passed.
- [Platform shells](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40996494510): passed (frontend Node tests/typecheck/build, Android shell and native Tauri tests).
- [Desktop launch smoke](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40996494485): passed.

## Limitations and remaining acceptance

This is a privacy-epoch-coherent native read, **not** an explicit SQLite snapshot transaction against all same-generation bookmark/visit updates. Cross-process deletion immediately after the final check can still be displayed until the next privacy poll; no synchronous cross-process push is implied. Tests use synthetic host processes/SQLite handles and frontend parsing, not a real mounted desktop browser or physical Android device. Android personal-data adapters, complete backup/restore lifecycle and device acceptance remain open. **STATE-001 remains unchecked**.
