# SRC-003: Live official inventory + complete local generation integrity (2026-10-26)

Status: scoped source acquisition verification; **SRC-003 remains unchecked**.

Host-only read-only command:

```text
wiki-pack verify-public-generation-live [--json] PROJECT GENERATION INVENTORY_SHA256 PRIVATE_SOURCE_DIR MAX_MEMBER_BYTES
```

The command first verifies the entire private staged dump generation: independently pinned `SHA256SUMS`, exact declared compressed members, their complete SHA-256, byte bounds and a clean tree with no symlinks, hardlinks, unlisted entries or orphan checkpoints. Only after local verification succeeds does it fetch the official `SHA256SUMS` over the existing TLS-validated, no-redirect `dumps.wikimedia.org` transport and compare it with the pinned inventory. A changed or unavailable upstream inventory fails closed, as do missing/corrupt local members. It never downloads members, modifies the staging tree, installs packs or activates snapshots.

The optional JSON result is schema 3, kind `public_dump_live_https_and_local_integrity`. It preserves the full schema-v2 per-shard receipt and adds `official_https_inventory_retrieved` and `live_inventory_matches_local_pin` checks. **No successful receipt** is printed on failure, and `publisher_authenticated` remains `false`.

The HTTPS result is **observed official TLS origin at check time**, not a signed Wikimedia publication, an independently authenticated publisher statement or independent page-census completeness. Revision-matched rendering, offline visuals and device acceptance are still missing. As with previous private-stage verification, untrusted concurrent writers must be excluded.

Synthetic two-shard regression covers missing/corrupt local inputs failing before HTTP, live exact-match receipt, changed/failed HTTPS inventory, unchanged staged data, canonical member ordering and conservative trust claims.
