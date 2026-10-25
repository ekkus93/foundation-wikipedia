# SRC-003: Idempotent verified-member reuse (2026-10-25)

Status: scoped software improvement; **SRC-003 remains unchecked**.

The host-only `wiki-pack fetch-public-member` path checks whether the exact
SHA256SUMS-declared compressed shard already exists in its private source tree
before spawning curl. Cached bytes must pass bounded streaming SHA-256
verification. The cache preflight also rejects symlinked paths, unsafe
intermediate directories, hard-linked target files and file identity changes
during opening. An occupied but corrupted shard fails closed and is not
overwritten or automatically trusted.

A missing member still uses the existing certificate-verified official HTTPS
transport, with no redirects, bounded output and HTTP 200 required before
atomic checksum-verified publication. This avoids repeated large downloads
after successful prior runs, but does not resume interrupted partial transfers
or authenticate the caller's inventory pin.

Synthetic regression proves a verified member succeeds even when curl would
fail, and a corrupted occupied member is rejected without overwriting it.

## Remaining acceptance

CI evidence is tracked against the exact source commit in the canonical TODO.
Live Wikimedia publisher provenance, partial-transfer resume, independent
census completeness, revision-matched rendering, required offline media and
desktop/Android acceptance remain outstanding. Do not mark SRC-003 done.
