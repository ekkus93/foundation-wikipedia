# STATE-001 — Concurrent SQLite WAL open (partial)

**Date:** 2026-10-29 (America/Los_Angeles)  
**Source SHA:** `8a6a00f12131481438338ff5754135a5505d75c6`  
**Status:** Partial; STATE-001 remains unchecked.

## Failure observed

The exact-head documentation commit `6d51d9dc3674147f2939144794dd6b23fb6e2c29` triggered the existing eight-thread `user_state::tests::parallel_initializers_publish_one_complete_schema` regression. [Rust workspace run 40989270248](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40989270248) failed with `SqliteFailure(DatabaseBusy, "database is locked")` in `UserState::open`. This test has intermittently failed on earlier heads; it was not caused by the preceding documentation changes.

## Correction

`crates/wiki-store/src/user_state.rs` now queries the existing journal mode first. Connections already using WAL do not re-issue `PRAGMA journal_mode=WAL`, avoiding an unnecessary exclusive transition. A bounded retry (at most 50 additional 20-ms delays, in addition to SQLite's existing busy timeout) handles only SQLITE_BUSY, SQLITE_LOCKED, or an incomplete mode transition while concurrent first-open initializers race. Invalid schemas and unrelated SQLite failures still fail closed. The original eight-thread regression continues to exercise concurrent schema publication and first-open writes.

## Exact-head CI evidence

- [Rust workspace 40989518265 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40989518265), including `wiki-store` tests, formatting and strict Clippy.
- [Platform shells 40989518289 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40989518289), including native desktop adapter qualification.
- [Android Rust ABI 40989518385 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40989518385).
- [Android codec probe 40989518380](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40989518380) was still running at documentation and is not SQLite open evidence.

## Remaining

A passing concurrency regression is not a formal stress benchmark or proof of no lock contention under all process interleavings. Physical desktop/Android lifecycle testing, full restore and migration flows, cross-process UI push and offline-pack acceptance remain open. Do not mark STATE-001 complete from this slice alone.
