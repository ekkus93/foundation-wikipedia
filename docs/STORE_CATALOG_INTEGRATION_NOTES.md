# Immutable SQLite catalog — STORE-001 integration notes

Status: **partial implementation**, not yet production-qualified (2026-10-10).

The Rust `wiki-store::catalog::SnapshotCatalog` indexes immutable `FWREC001`
length-prefixed, individually zstd-compressed MessagePack records. The catalog
maps project/page ID, canonical title and aliases, revision ID, optional
Wikidata ID, shard basename, byte offset, frame length and SHA-256 digest.
Direct-offset lookup reads and decompresses **only one addressed record**;
record identity, revision and digest are checked again on read. Redirect
resolution now rejects missing targets, cycles and chains beyond 32 verified
records; this does not prove full-snapshot coverage.

The provisional Python `scripts/build_snapshot_catalog.py` tool builds a
SQLite catalog from a caller-provided NDJSON index proposal and verifies the
on-disk frame header, declared lengths and digest. The proposal is not
authenticated Wikimedia provenance, and the Python builder does **not** decode
and independently verify canonical `PageRecord` identity. Its index must not
be activated as trusted content solely because SQLite creation succeeded.
The Rust decoder provides the stronger identity-bound read path.

## Trust and activation boundary

A SQLite index is a derived lookup accelerator, not a publisher signature,
snapshot receipt, or proof that every required article and visual is present.
Before snapshot activation, the pipeline still needs to verify the complete
official source inventory, canonical normalized records, all referenced
assets, index-to-object coverage, source/revision joins and applicable pack
signatures. Catalog database files belong in immutable snapshots; bookmarks,
history, user pins and chat history belong in separate mutable user storage.

The current tests cover bounded direct-offset reads, duplicate-title
transaction rollback, wrong-offset/missing-record handling, digest tampering,
and title/Wikidata lookup. Production qualification still requires large
representative snapshots, corrupt/missing/symlink shard fault injection,
Android random-access/memory acceptance, restart recovery, and full
source-to-installed-reader end-to-end tests. Leave `STORE-001` unchecked
until those criteria are evidenced.

## Follow-on STORE-002 ownership contract

Media bytes should be addressed by content hash and deduplicated across
packs. Store individual source/creator/license/attribution notices separately
from the shared object bytes. Keep explicit pack, temporary cache and user
pin ownership distinct; deleting one pack must never delete an object still
referenced by another pack or a user pin. Audio/video payloads are on-demand
online resources, while preview imagery and metadata remain offline.
