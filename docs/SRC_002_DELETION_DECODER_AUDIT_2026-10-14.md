# SRC-002 deletion decoder review (2026-10-14)

`decode_pinned_deletions` currently accepts repeated page IDs with matching scope and digest. Its API documentation promises that duplicate records are rejected. The downstream joined importer rejects duplicates, but callers using the public decoder alone can receive duplicate markers. Add decoder-level duplicate tracking, a typed duplicate error, and direct public-API regression tests. Do not mark SRC-002 complete: authenticated upstream completeness and required media remain outstanding.
