# STORE-001 / STORE-004 / DUI-001 — Borrowed verified reader coverage (2026-10-28)

## Defect and correction

The public `OfflineReader::new_verified` borrowing constructor previously checked only derived FTS catalog membership. This did not prove complete canonical title/alias mappings or reject trailing bytes outside indexed immutable frames. The owned `VerifiedOfflineCatalogReader::open` already enforced the stronger `SnapshotCatalog::verify_shard_coverage` preflight, but callers using the borrowing constructor could skip it unintentionally.

The borrowing constructor now requires `catalog.verify_shard_coverage(root)?` before `index.verify_catalog_membership(catalog, root)?`. This verifies contiguous frame coverage, catalog identities and digests, title/alias completeness, absence of orphan title mappings, and no trailing shard bytes before returning a reader. No publisher authentication or mutation is performed.

## Synthetic regression and exact-head qualification

The existing disk-backed reader fixture deletes a canonical alias while FTS membership remains valid, then restores it. It separately appends an unindexed byte after the final shard frame while FTS membership remains valid, then restores the original shard. Both defects now cause `OfflineReader::new_verified` to fail closed with a catalog error, matching the owned reader. Healthy snapshots continue to open. The pre-existing frame-corruption regression now expects the catalog preflight's `DigestMismatch` rather than the former search-wrapper error.

Initial implementation `c7d855b018f77f8e42e977b0e6e148e2932e9331` failed the old error-variant assertion ([Rust CI 40803856481](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40803856481)). Corrected source `13c6d606853f63aaf8d0d5906837b6ffa55cfe12` passed [Rust workspace 40804061748](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40804061748), [Platform shells 40804061578](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40804061578), and [Android Rust ABI 40804061712](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40804061712). Android record codec probe 40804061627 was still running at the documentation observation.

## Boundaries

This is a local, synthetic catalog completeness check, not an independently authenticated Wikimedia source, signed publisher pack, complete mandatory image/math/media/attribution inventory, installed-pack activation, production desktop/Android UI, or real-device airplane-mode acceptance. The snapshot must remain immutable during the reader's lifetime. **STORE-001, STORE-004 and DUI-001 remain unchecked.**
