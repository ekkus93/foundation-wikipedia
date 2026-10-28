# STORE-004 — Complete derived FTS catalog membership audit (2026-10-28)

## Problem

Verifying only the immutable frame for each returned search hit cannot detect an omitted article in the derived SQLite FTS5 index. A corrupted or stale FTS table can return plausible, digest-verified matches while silently excluding other installed articles, especially across multiple Wikipedia editions.

## Implementation

Added `OfflineSearchIndex::verify_catalog_membership(&SnapshotCatalog, &Path)` in `crates/wiki-store/src/search_index.rs`. The read-only audit walks all catalog records and FTS rows in project/numeric-page-ID order, verifies every catalog frame, and requires exactly one matching FTS article row with the same project, stable page ID, revision ID and canonical title. Redirect records are verified but must not become separate search hits. Missing, extra, duplicate and mismatched entries fail closed with `SearchError::StaleIndex`. A 100,000-article limit bounds the audit; the scan does not materialize the full corpus in memory.

The ordering explicitly casts FTS5's unindexed page-ID column to integer. This prevents lexicographic ordering (page 10 before page 2) from creating false mismatches. The caller must independently authenticate the source and verify the snapshot/pack before treating any content as installed.

## Regression and verification

The synthetic `complete_catalog_membership_audit_rejects_missing_forged_and_stale_fts_rows` regression creates three immutable articles across English and French editions, inserts frames in non-key order, verifies successful complete membership, then checks that deleted rows, stale revisions, forged titles, duplicate rows and a corrupt immutable shard all fail closed. It exercises both numeric page ordering and edition separation.

- Initial implementation SHA `c1bb1f38caecd2dd2e73213945e6ee6bff843ddb` and first formatting repair `a0da260e9d8d03ddb4e988cfce24dce0a99b840c` failed rustfmt; corrected SHA **`ddcfe5d32cd41040853d72c4e5debc6081010255`** passed.
- [Rust workspace CI 40737252886](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40737252886): passed (format, strict Clippy and workspace tests).
- [Android Rust ABI CI 40737252947](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40737252947): passed.
- [Platform shells CI 40737252841](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40737252841): running at documentation.
- [Android record codec probe 40737252828](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40737252828): running at documentation.

## Remaining qualification

This is a **read-only derived identity membership audit**, not a complete FTS integrity proof: it does not compare token postings or every indexed body byte against canonical text. The audit is an explicit API and is not yet wired into a production pack activation or reader opening gate. Authenticated complete source imports, mandatory offline visual media, installed-pack integration, desktop/Android navigation and real-device airplane-mode acceptance remain open. **STORE-004 stays unchecked.**
