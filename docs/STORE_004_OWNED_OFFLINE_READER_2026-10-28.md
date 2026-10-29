# STORE-004 / DUI-001 — Verified owned offline catalog reader (2026-10-28)

## Integration primitive

The shared Rust `wiki_store::offline_reader::VerifiedOfflineCatalogReader` owns its `SnapshotCatalog`, `OfflineSearchIndex` and immutable snapshot-root pathname. `open(catalog_path, index_path, snapshot_root)` opens an existing SQLite catalog read-only and requires `OfflineSearchIndex::open_verified_read_only`: complete digest-verified catalog membership and canonical bytes, plus a native FTS5 shadow/posting integrity check via a private writable clone. `reader()` lends a local-only `OfflineReader` without a second activation-time scan. This reduces the chance of a product adapter accidentally using a weaker unverified constructor.

## Regression

A disk-backed article and redirect fixture proves alias navigation and edition-scoped search through the owned reader. Raw corruption of FTS5's `docsize` shadow records is rejected at the owned entry point; rebuilding only derived FTS data permits opening again. No upstream download, HTTP or physical device is involved.

## Boundaries and next integration

This is not yet an installed-pack activation API. The caller MUST authenticate source snapshot provenance, validate every required local visual and its attribution, bind the selected official/custom pack and ownership to a consistent activation state, and ensure source files cannot change during the reader's lifetime. Desktop Tauri and Android UniFFI/WebView are NOT yet wired to this constructor; production installed/offline claims remain disallowed. Local scratch space of roughly one FTS index is required for an activation-time native audit. **STORE-004, DUI-001 and MOB-002 remain unchecked.**

Exact-head CI evidence must be checked for the implementation SHA before recording acceptance.
