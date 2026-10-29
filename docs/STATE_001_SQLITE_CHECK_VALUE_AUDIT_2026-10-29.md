# STATE-001 — Explicit persisted SQLite CHECK-value audit (2026-10-29)

## Scope and failure evidence

The existing SQLite state startup and restore gates ran `PRAGMA quick_check(1)` and `PRAGMA integrity_check` respectively, plus a separate foreign-key audit. A disk-backed regression inserted a negative `reading_positions.scroll_offset` after `PRAGMA ignore_check_constraints=ON`. At original master `b5e8bf586ea4ac267d4ec483265d40d4cb06b568`, the reopen path rejected the database but the restore path accepted it and published a restored copy. [Failing Rust CI](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40937556319) identifies `reopen_rejects_existing_check_and_foreign_key_corruption` and `accepted corrupt user rows on restore: violated-check`. SQLite integrity PRAGMAs alone are not a portable substitute for explicitly checking these application invariants.

## Implementation

The shared `validate_persisted_checks` gate rejects bookmark rows with `page_id <= 0` or `revision_id <= 0`, and reading-position rows with `page_id <= 0`, `revision_id <= 0`, or `scroll_offset < 0`. It is invoked before v1 migration, after current-schema initialization, and during read-only backup preflight **before** any restore destination is created. Existing schema-shape and foreign-key checks remain mandatory. Tests cover invalid bookmark page/revision IDs, invalid reading-position revision and scroll offsets, and orphaned chat rows; they require a rejected reopen/restore and no output destination.

## Exact-head verification

- Initial source commit `18b43195141a8b96b700c1f5b5e489efe8f80353`: [Rust workspace 40940048440](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40940048440) failed only `cargo fmt --all -- --check` due to a blank line.
- Corrected implementation commit `d7b45aabc655bff0821e2ea9ae83bd3d3e8045bc`: [Rust workspace 40940163045](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40940163045) **passed**, including strict Clippy and tests; [Android Rust ABI 40940163091](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40940163091) **passed**. [Platform shells 40940163137](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40940163137) and [Android record codec probe 40940163301](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40940163301) were still running when this evidence was recorded.

This is a scoped integrity regression fix. It does not establish full STATE-001 acceptance, real-device behavior, or a complete privacy/settings workflow. Leave STATE-001 unchecked.
