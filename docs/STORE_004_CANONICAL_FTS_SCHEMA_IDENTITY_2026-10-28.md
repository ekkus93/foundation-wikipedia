# STORE-004 — Canonical FTS5 schema identity (2026-10-28)

## Problem

The offline search opener previously accepted any SQLite virtual table whose DDL began with `CREATE VIRTUAL TABLE` and contained `USING fts5(`. Porter stemming, reordered `title`/`body` columns, indexed identity columns and contentless storage could pass despite violating expected search, ranking or body-audit semantics. Writable initialization used `IF NOT EXISTS` without checking the existing schema.

## Implementation

`crates/wiki-store/src/search_index.rs` now requires the exact canonical SQLite FTS5 virtual-table DDL for both read-only opening and writable initialization. It fixes column order, `UNINDEXED` identity fields, and the `unicode61` tokenizer. An incompatible existing table fails with `SearchError::StaleIndex` before queries or rebuilding. SQLite normalizes the `IF NOT EXISTS` creation statement to the canonical stored DDL.

## Tests

`lookalike_fts_schema_is_rejected_before_read_or_rebuild` constructs four valid SQLite FTS5 databases using incompatible stemming, swapped columns, indexed identity fields or contentless storage. Both opening paths reject each. Existing canonical read-only/open tests exercise valid schema acceptance.

## Limitations

This validates schema identity, not internal FTS token-posting correctness or shadow-table integrity. Publisher-authenticated source completeness, required offline media, installed-pack integration and physical-device acceptance remain outstanding. **STORE-004 remains unchecked.**

## Exact-head verification

The initial implementation `6773d056c23af319b79df5fae23091a351c04484` failed the Rust formatting gate ([40773963080](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40773963080)). The one-line rustfmt correction `e0cfa74fd615458d54f035c383a1f65eb043379d` passed:

- [Rust workspace 40774064572](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40774064572) — formatting, strict Clippy, workspace tests.
- [Platform shells 40774064635](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40774064635) — passed.
- [Android Rust ABI 40774064776](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40774064776) — passed.
- [Android record codec probe 40774064543](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40774064543) — still running at last observation, not counted as passed.
