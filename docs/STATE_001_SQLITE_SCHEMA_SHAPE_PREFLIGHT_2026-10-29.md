# STATE-001 — SQLite schema-shape preflight (2026-10-29)

## Scope

The mutable SQLite user-state database is separate from immutable Wikipedia snapshots. The previous current-schema gate checked required table and index **names** but could accept an unrelated table with a forged name or a missing chat foreign-key cascade. A backup restore could copy such a database to a new path before its incompatibility became apparent.

At source commit `0526453b76f40ad13209fb757544877e4829c36a`, `crates/wiki-store/src/user_state.rs` adds a shared schema-shape audit for:

- All eight expected mutable-state tables, with ordered column names and declared SQL types.
- Four required secondary indexes for schema version 2.
- The `chat_messages.thread_id -> chat_threads.id` foreign key with `ON DELETE CASCADE`.
- Unexpected SQL views and triggers, which must not execute as a side effect of later personal-state writes.

The audit runs before exposing a current-version database, before migrating an existing version-1 database, and before creating a destination for a restored backup. Fresh schema creation and supported v1 migration remain available.

## Regression evidence

A disk-backed regression mutates an otherwise valid database three ways: rename the bookmark anchor column, add a trigger to settings, and replace the chat-message table without its cascade. Each forged schema is rejected as `UserStateError::InvalidValue` both by `UserState::open` and by `restore_backup_to_new_path`. Failed restores do not create a destination file. Existing backup/restore, v1 migration, and current-schema index/table presence tests remain in the Rust workspace suite.

The initial implementation commit `60ad15ac9f7045ed2cdb0c2b255b95f5996415ee` failed formatting. A follow-up `b5bd5e24884701c625ae418d23feb2f0c27ecafb` accidentally truncated the final test and failed parsing. Both were repaired at the qualified exact source SHA.

- [Rust workspace CI — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40925104758)
- [Platform shells CI — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40925104931)
- [Android Rust ABI CI — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40925105207)
- [Android record codec probe](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40925104766) — in progress at documentation.

## Qualification boundary

This verifies expected column names and types, index presence, foreign-key cascade, and absence of triggers/views. It does **not** prove every `CHECK`, `NOT NULL`, primary-key definition or index expression, validate all persisted row semantics, prevent concurrent edits to an untrusted backup, authenticate upstream Wikimedia content, or qualify real desktop/Android devices. `STATE-001` remains unchecked until its complete acceptance requirements are met.
