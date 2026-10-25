# SRC-003: End-to-end resumable generation acquisition (2026-10-25)

Status: partial public-dump source transport; **SRC-003 remains unchecked**.

The host-only `wiki-pack fetch-public-generation [--json]` operator command
combines the existing exact independently SHA-256-pinned official HTTPS
`SHA256SUMS` acquisition, bounded HTTP 206 resumable download of every
inventory-declared shard in canonical page-range order, and the full
read-only local tree/size/SHA-256 verification gate. The supplied private
source directory must already exist and be private (mode 0700 on Unix).

A successful `--json` invocation emits **one** schema-v2
`public_dump_local_integrity` receipt with all per-shard digests and byte
counts. Errors emit no successful receipt on stdout. Partial private
checkpoints may survive failed transfers for retry; no pack is installed
or active snapshot modified. Repeated invocation on a fully staged source
can complete without spawning curl, while the complete tree and each shard
are reverified. An invalid per-shard byte limit fails before HTTP.

Synthetic tests exercise two-shard interrupted HTTP-range acquisition,
resuming only the missing suffix, deterministic offline replay, clean-tree
enforcement and early invalid-limit rejection. A black-box CLI test
uses an intentionally invalid PATH to prove complete staged generations
need no network client, checks JSON and human output, and rejects incorrect
pins and unexpected checkpoint files without a success receipt.

**Trust boundary:** The independently supplied checksum is an integrity pin,
not an authenticated Wikimedia publisher signature or proof of full
generation census. This command does not prove revision-matched HTML,
mandatory offline media, an installable official pack, or desktop/Android
device acceptance.
