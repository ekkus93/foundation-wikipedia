# SRC-003 — bounded checksum pass over filtered XML (2026-10-15)

**Status:** Narrow diagnostic-import scalability improvement. SRC-003 remains unchecked.

## Observed implementation risk

The diagnostic public-dump renderer already streams input bzip2 lines into a
filtered XML file, but its final SHA-256 audit called `handle.read()` without a
size cap. For a filtered export containing large MediaWiki/Template/Module
namespaces, this may allocate a buffer proportional to the *entire filtered
file*. That contradicts the bounded-memory objective of large-dump processing.

## Correction and regression

The output digest is now computed through `_sha256_stream` with explicit
1-MiB `read(size)` windows. SHA-256 over the same byte sequence is unchanged;
the signed/verified source-member checksum and the original-source provenance
checks remain separate. A deterministic 2-MiB-plus fixture exercises multiple
reads, refuses omitted or unbounded `read()` sizes and verifies the canonical
digest. Empty-file digest is covered as well.

This only bounds memory for the checksum pass. Filtering one large page still
builds its page body in memory, and the code remains a diagnostic Python
importer—not the portable Rust XML/bzip2 fallback. It does not authenticate
complete upstream census, import File namespace/media binaries, verify license
or attribution or establish offline pack readiness.

## Qualification

Run exact-head Rust workspace (Python fixture discovery) and Platform shells
CI after the direct-master commit. No checklist item is marked complete on
this narrow reliability change.
