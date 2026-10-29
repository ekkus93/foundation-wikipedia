# STATE-001 — SQLite constraint and index-definition preflight (2026-10-29)

## Scope

The mutable SQLite database is separate from immutable Wikipedia snapshots. Earlier validation checked table column names/types, required index names, the chat-message cascade and absence of executable triggers/views. A forged database could still preserve those names while changing a primary key, allowing NULL in required columns or replacing an expected index with an expression or different sort order.

At corrected source commit `f6c7d5672ceabb8fb672e96237b30fac9e99ce05`, `crates/wiki-store/src/user_state.rs` now checks:

- Primary-key ordinals and declared NOT NULL flags for all eight state tables. It explicitly accounts for SQLite reporting NOT NULL=0 on INTEGER PRIMARY KEY and legacy TEXT PRIMARY KEY columns without an explicit constraint.
- For each of four version-2 indexes, the expected owning table, nonunique and nonpartial status, explicitly created origin, exact key column sequence, ascending/descending order and binary collation.
- Expression-index keys are rejected as invalid schema instead of being misclassified as a SQL decoding error.

These checks run on startup, before v1 migration where applicable, and before restoring a backup into a new path. They do not mutate the source database on failure.

## Regression and CI

Disk-backed tests create a valid state database, then independently alter its schema to permit a nullable builder-job status, change the bookmarks composite primary key, substitute the history index's column, reverse its sort order, make it unique or introduce an expression key. Every tampered database must be refused both by `UserState::open` and by `restore_backup_to_new_path`; no restored destination is created. Existing healthy initialization, v1 migration and backup/restore tests remain in the suite.

- **Qualified implementation:** `f6c7d5672ceabb8fb672e96237b30fac9e99ce05`
- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40935692141)
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40935691994)
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40935692000)
- [Android record codec probe](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40935692388) — still in progress at qualification.

Initial code `52f30799fe400ebef91f88383f5b9c9e3df41bbd` failed rustfmt. Formatting repair `295d79580157a9add3fcb53548ec75d951b1961a` failed strict Clippy's type-complexity gate. Named-type repair `45f0941f2e996a74c685f3dbde1347c913869d78` exposed that expression-index NULL metadata was classified as a SQL error rather than InvalidValue; the corrected source passed the above workflows.

## Qualification boundary

This is a schema-shape and query-index integrity gate, not an audit of every CHECK constraint, stored row value, Windows ACL, cross-process backup mutation, device restart or complete application acceptance. The parent `STATE-001` remains unchecked.
