# SRC-003: Idempotent pinned SHA256SUMS reuse (2026-10-25)

Status: partial source acquisition; **SRC-003 remains unchecked**.

The host-only `wiki-pack fetch-public-inventory` command now reuses an
existing digest-pinned `SHA256SUMS` file in the caller's private staging
directory, without network transfer on retry. Before reuse the complete
inventory is reread and revalidated against the separately supplied SHA-256,
Wikimedia project and release generation. File type, size, hard-link count and
pre/post-open file identity are checked by the same gate as member fetching.

A missing inventory still uses certificate-verified official HTTPS and atomic
no-overwrite publication. An occupied mismatched, tampered, symlinked or
hardlinked inventory fails closed and is never overwritten. Synthetic tests
exercise idempotent retry without transport and unsafe occupied files.

An independently supplied digest does NOT establish Wikimedia publisher
authentication, source-generation completeness, revision-matched rendering,
required offline visuals or desktop/Android acceptance. The active snapshot
is not changed by this command.
