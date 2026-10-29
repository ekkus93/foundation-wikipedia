# STATE-001 — Cross-process SQLite schema migration serialization (2026-10-29)

## Problem and implementation

Disk-backed personal state already used SQLite WAL and a five-second busy timeout, but `UserState::initialize` read `PRAGMA user_version` before obtaining a write transaction. Two processes could observe v1 (or a fresh v0), then independently attempt the same table/index migration; the second process could encounter duplicate-object SQL errors or reject an otherwise healthy database. Schema preflight could also be performed against a generation different from the one ultimately migrated.

`crates/wiki-store/src/user_state.rs` now opens one `TransactionBehavior::Immediate` transaction before version discovery. It keeps that shared SQLite writer lock across version/foreign-key/integrity checks, v0 table creation, v1-to-v2 index creation, final schema/row validation and commit. Errors roll back the whole transaction. A concurrent opener waits for the first writer and then reads the newly committed schema version, rather than relying on an earlier stale observation. WAL remains enabled for disk-backed state after initialization; in-memory behavior is preserved.

## Automated regression evidence

- `concurrent_schema_upgrade_rechecks_version_after_writer_lock`: a disk-backed v1 database containing a saved bookmark is upgraded by an independent connection holding an uncommitted `BEGIN IMMEDIATE` transaction. A second opener waits, then successfully opens the committed v2 database and reads the retained bookmark. The test checks that the opener does not return before the first writer commits.
- `parallel_initializers_publish_one_complete_schema`: eight synchronized threads independently open a fresh disk-backed database, confirm schema version 2 and write a setting. The final reopen verifies the persisted setting and valid complete schema.
- Existing v1 migration, corrupt-schema rejection, backup/restore, and multi-handle WAL tests are exercised by the workspace suite.

First source commit `9cc914816388d87b5a3ff9f86d777a84a02aa781` failed the Rust workspace formatting gate: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40960883621 . Corrected exact source commit `26fb6d1727de627fedb36cba86d8963e04cf88a3` passed:

- Rust workspace (fmt, strict Clippy, unit tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/40960998885
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40960998957
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40960998983 (in progress at documentation)
- Android record codec probe: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40960998805 (in progress at documentation; not specific to user-state)

## Boundaries and outstanding acceptance

This is a SQLite-level schema initialization/migration lock, not a general multi-process personal-data generation protocol. Other processes can still perform user-state writes before or after a privacy reset without an application-level cross-process fence. Backup source identity, owner-private directory/Windows ACL policy, restore coordination, real desktop/Android process-restart behavior and physical-device qualification remain open. The canonical `STATE-001` top-level task and its broad subtask acceptance remain unchecked until those requirements are satisfied.
