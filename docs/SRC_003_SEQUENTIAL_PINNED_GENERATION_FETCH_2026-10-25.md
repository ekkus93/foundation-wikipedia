# SRC-003: Sequential pinned generation acquisition (2026-10-25)

Status: partial software qualification; SRC-003 remains unchecked.

After staging an independently pinned SHA256SUMS with
`wiki-pack fetch-public-inventory`, operators can now run:

`wiki-pack fetch-public-members PROJECT GENERATION INVENTORY_SHA256 PRIVATE_SOURCE_DIR MAX_COMPRESSED_BYTES_PER_MEMBER`

This command validates the private inventory once and stages every declared
member in ascending page-range order using the existing certificate-verified
HTTPS, HTTP-200, SHA-256 and atomic no-overwrite member transport. It stops
at the first failed shard, preserving previously verified shards. Repeating
the command rehashes and reuses already complete shards rather than
downloading them again. It does not automatically trust or replace an
occupied corrupted member.

A synthetic two-member test verifies sorted ordering, single-inventory
preflight, early stop on failure and zero transport on invalid generation,
digest or byte budget.

This is **not** resumable partial-file transfer or independent evidence that
the checksum publication is genuine or complete. The source still needs
independent publisher/census provenance, revision-matched article rendering,
mandatory offline media and real desktop/Android acceptance.
