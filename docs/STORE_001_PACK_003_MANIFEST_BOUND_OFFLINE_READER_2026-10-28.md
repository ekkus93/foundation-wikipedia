# STORE-001 / PACK-003 — Manifest-bound owned offline reader (2026-10-28)

## Implementation

`VerifiedOfflineCatalogReader::open_with_manifest_shards` accepts the complete expected shard name, byte-size and SHA-256 inventory from a **previously authenticated** pack manifest. It opens the installed catalog read-only, audits contiguous catalog frame and title/alias coverage, requires exact expected shard membership, streams and hashes every complete shard against the supplied inventory, then verifies the published FTS index's canonical body/redirect coverage and native FTS5 integrity before exposing a reader. No network request or mutation of published bytes is involved.

## Synthetic regression

The disk-backed two-record reader fixture supplies an exact SHA-256 inventory and verifies local navigation through the new constructor. An empty inventory, wrong digest and wrong declared shard length fail closed. Existing FTS shadow corruption, missing canonical alias, unindexed trailing shard bytes, redirect identity and revision-bound regressions remain.

## Scope

The method **does not authenticate the manifest's publisher** or discover whether all required images, maps, math, captions, references and attribution were declared. Caller-provided inventory must already have authenticated provenance; activation, media completeness and real desktop/Android airplane-mode acceptance remain separate gates. The snapshot must be immutable throughout the reader lifetime. **STORE-001 and PACK-003 remain unchecked.** Record exact source CI in the canonical TODO.
