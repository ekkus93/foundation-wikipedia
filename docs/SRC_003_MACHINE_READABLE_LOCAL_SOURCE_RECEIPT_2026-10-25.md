# SRC-003: Machine-readable local source integrity receipt (2026-10-25)

## Scope

`wiki-pack verify-public-members --json <project> <generation> <SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>`
performs the same read-only full-generation integrity verification as the
human-readable command, then emits exactly one deterministic JSON object
on stdout. On any verification error, it emits no receipt and exits nonzero.

The version-1 receipt includes the project/generation, normalized inventory
SHA-256 pin, verified member count, verified aggregate compressed bytes,
explicit successful local integrity checks and explicit **false** flags for
publisher authentication, independent census, revision rendering, required
offline media and pack installation. The receipt is an unsigned local
diagnostic, NOT an authoritative Wikimedia publication or an installation
authorization. It must not be used to mark a pack offline-ready.

This output allows local orchestration to distinguish verified acquisition
bytes from the separately required provenance, page-census, rendering, media
and device acceptance gates without parsing human-readable CLI prose.
The command does not download data, modify source files or change the
installed snapshot.

Synthetic regression covers missing members, exact JSON fields, digest
case normalization, deterministic output, full-checksum corruption and
explicit false trust flags. This increment does not complete SRC-003.
