# SRC-003 — Per-member machine-readable local integrity receipt (2026-10-25)

## Scope

The host-only `wiki-pack verify-public-members --json` command now emits
**schema version 2** with `verified_members`: one record per independently
digest-checked compressed shard, ordered by its canonical numeric page range.
Each record includes a zero-based index, inventory-declared relative path,
lowercase SHA-256, and actual verified compressed byte count. Aggregate count
and byte totals still reflect the same verification pass; the command does
not trust the inventory's filenames or checksums without rereading each shard.

The full-tree structural preflight, private path checks, checksum verification,
no-output-on-failure behavior and human-readable command remain unchanged.
Synthetic tests cover reversed SHA256SUMS line order, uppercase inventory
checksums, exact per-shard bytes and corruption of a later member.

The version bump is intentional: downstream receipt consumers must validate
`schema_version` and reject unsupported versions instead of interpreting
the additional member evidence as authenticated publisher provenance.

## Trust boundary

These receipts establish **local digest-pinned transport integrity only**.
They do not authenticate Wikimedia's publication, establish an independent
page-ID census, prove exact-revision HTML fidelity, include mandatory offline
visuals or install/activate a pack. `SRC-003` remains unchecked.
