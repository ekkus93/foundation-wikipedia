# STORE-001 / DUI-001 — Verified offline reader catalog coverage (2026-10-28)

## Change

The owned `VerifiedOfflineCatalogReader::open` now requires `SnapshotCatalog::verify_shard_coverage` before publishing a reader. The existing FTS identity, canonical-body, redirect-suffix and native FTS5 integrity audits do not by themselves prove contiguous shard framing or complete title/alias index membership. The catalog audit checks every indexed immutable frame, contiguous shard bytes, canonical title/alias rows and orphaned derived mappings, and fails closed before exposing the reader.

## Synthetic regression

The existing disk-backed reader fixture first proves normal read-only construction. It then deletes a canonical alias row while leaving all indexed article frames and FTS content intact, verifies that the FTS-only opening still passes, and requires the owned reader to reject the catalog. After restoration, it appends an unindexed byte to the shard, again demonstrates that the FTS-only audit passes and requires owned opening to reject the trailing bytes. Both cases recover when the canonical state is restored. No external publisher, live Wikipedia, or physical device is involved.

## Acceptance boundary

This is a local consistency and publication preflight, not proof of an authenticated Wikimedia source generation, complete required visual/media/attribution inventory, pack activation, or production desktop/Android integration. The snapshot directory must remain immutable during and after opening. **STORE-001 and DUI-001 remain unchecked.**

Exact-head CI must be recorded in the canonical TODO before this slice is considered qualified.
