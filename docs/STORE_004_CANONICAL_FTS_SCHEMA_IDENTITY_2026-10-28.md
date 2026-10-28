# STORE-004 — Canonical FTS5 schema identity (2026-10-28)

## Problem

The offline search opener previously accepted any SQLite virtual table whose DDL began with `CREATE VIRTUAL TABLE` and contained `USING fts5(`. Porter stemming, reordered `title`/`body` columns, indexed identity columns and contentless storage could pass despite violating expected search, ranking or body-audit semantics. Writable initialization used `IF NOT EXISTS` without checking the existing schema.

## Implementation

`crates/wiki-store/src/search_index.rs` now requires the exact canonical SQLite FTS5 virtual-table DDL for both read-only opening and writable initialization. It fixes column order, `UNINDEXED` identity fields, and the `unicode61` tokenizer. An incompatible existing table fails with `SearchError::StaleIndex` before queries or rebuilding. SQLite normalizes the `IF NOT EXISTS` creation statement to the canonical stored DDL.

## Tests

`lookalike_fts_schema_is_rejected_before_read_or_rebuild` constructs four valid SQLite FTS5 databases using incompatible stemming, swapped columns, indexed identity fields or contentless storage. Both opening paths reject each. Existing canonical read-only/open tests exercise valid schema acceptance.

## Limitations

This validates schema identity, not internal FTS token-posting correctness or shadow-table integrity. Publisher-authenticated source completeness, required offline media, installed-pack integration and physical-device acceptance remain outstanding. **STORE-004 remains unchecked.**
