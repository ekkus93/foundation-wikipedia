# STORE-001 — Portable shard filename identity (2026-10-28)

## Problem

An immutable snapshot catalog can be produced on a case-sensitive filesystem with two distinct shard names that differ only by ASCII letter case. The same manifest may resolve both names to one physical file on case-insensitive Windows/macOS volumes. Exact byte hashes do not by themselves prevent such filename aliasing, and successful Linux-only activation must not be interpreted as portable acceptance.

## Implementation

The full `SnapshotCatalog::verify_shard_coverage` pre-activation audit now retains a set of ASCII-case-folded shard names and rejects a second distinct spelling with `CatalogError::InvalidCatalogEntry`. The check covers the complete SQLite shard inventory, before publication, and is inherited by `verify_expected_shards` and `verify_manifest_shard_hashes`. It does not reject ordinary repeated frame references to the **same** shard name.

## Regression and qualification

The Unix regression `shard_coverage_rejects_case_insensitive_filename_aliases` writes two distinct valid immutable frames under `Articles.shard` and `articles.shard`, verifies the single-shard catalog, inserts the second record, then requires full coverage and expected-shard verification to reject the ambiguous inventory. Run `cargo test -p wiki-store shard_coverage_rejects_case_insensitive_filename_aliases --locked` and the repository's full Rust CI. Record exact-head results in the next qualification update.

## Remaining

The source publication, full manifest trust chain, required offline media, pack installation, desktop/Android navigation and real filesystem/device acceptance remain separate. **STORE-001 is not complete.**
