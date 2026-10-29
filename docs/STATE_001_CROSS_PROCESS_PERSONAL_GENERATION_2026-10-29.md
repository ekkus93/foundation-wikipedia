# STATE-001 — Cross-process SQLite personal-data generation fence (2026-10-29)

## Scope and safety contract

The shared Rust `wiki-store::user_state::UserState` now supports durable privacy-generation tokens for queued personal-data writes across independent SQLite database connections. A caller captures `personal_generation()` when the user initiates an action, then supplies that exact token after queued/async work to the corresponding guarded API. If another process cleared personal data in the meantime, the guarded write returns `false` / `None`, without mutating cleared user state.

The persistence key `internal.personal_generation` is private to this mechanism; ordinary `set_setting` and `delete_setting` cannot change it. Missing keys in older schema-v2 databases represent generation 0. The value is a bounded canonical unsigned decimal integer. Overflow is an error, and no personal rows are cleared if the generation cannot advance.

`clear_personal_reading_data` now obtains a SQLite `BEGIN IMMEDIATE` transaction, advances the generation, deletes bookmarks/reading positions/history/chat threads (with chat-message cascade), then commits atomically. Existing installed-pack owners, builder jobs, and unrelated settings remain intact. SQLite writer serialization ensures that a guarded write cannot race across the reset's atomic generation update.

## Guarded APIs

- `set_bookmark_for_generation`: atomic upsert conditional on the captured generation.
- `record_visit_for_generation`: checks the captured generation and current history opt-in inside the same SQL insert statement.
- `set_reading_position_for_generation`: atomic revision/scroll upsert conditional on generation.
- `create_chat_thread_for_generation`: returns `None` when the generation is stale; a fresh insertion returns its new thread ID.

These APIs are **additive**. Legacy public unguarded writes continue to exist for backward compatibility and therefore do **not** receive cross-process stale-write protection. Application adapters must capture a token at the time of user intent and use these APIs consistently to obtain the claimed guarantee. This does not serialize unrelated processes' in-memory previews or forcibly cancel pending HTTP/AI work.

## Regression and exact-commit evidence

The disk-backed multi-handle regression captures generation 0 in one SQLite connection and clears personal data in another. Guarded writes with the old token cannot recreate bookmarks, history, reading position or chat threads. A fresh generation token permits new activity; installed object ownership survives. A separate test forces generation exhaustion and proves failed deletion preserves prior personal data. The reserve-key API is not writable through ordinary settings methods.

The original implementation `ab4056540cf08bd66c8a82285aecadb27c6246ef` failed rustfmt only. Corrected `2eea50598c781a82f03ffbe6e9abf2c5b8cf7b98` passed [Rust workspace 40963898274](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40963898274), [Platform shells 40963898328](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40963898328) and [Android ABI 40963898290](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40963898290).

The expanded code `6fcd41f374e174275305179cdcd9bc2f7c026a91` failed rustfmt only. Corrected implementation **`23a24e1e630fcc461f6f2fcf3193d59e11807845`** passed [Rust workspace 40964803362](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40964803362), [Platform shells 40964803342](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40964803342), and [Android ABI 40964803426](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40964803426). [Android record codec probe 40964803471](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40964803471) was still running at documentation; this probe is not direct privacy-reset evidence.

## Remaining work

Application integration, guarded chat-message appends/removals, explicit history-only clear generation coordination, cross-process native cache/session reset and Android physical qualification remain open. Older unguarded APIs must either be transitioned to generation-aware methods at application boundaries or explicitly documented as not privacy-safe after queued work. No end-to-end cross-process privacy guarantee or completed STATE-001 status is claimed.
