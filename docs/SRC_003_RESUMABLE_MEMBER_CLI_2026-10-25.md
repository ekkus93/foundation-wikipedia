# SRC-003: Resumable official-member CLI (2026-10-25)

Status: partial source acquisition; **SRC-003 remains unchecked**.

The host-only `wiki-pack` CLI now exposes two explicit opt-in commands:

```text
wiki-pack fetch-public-member-resume PROJECT GENERATION INVENTORY_SHA256 PRIVATE_SOURCE_DIR MEMBER_INDEX MAX_COMPRESSED_BYTES
wiki-pack fetch-public-members-resume PROJECT GENERATION INVENTORY_SHA256 PRIVATE_SOURCE_DIR MAX_COMPRESSED_BYTES_PER_MEMBER
```

As with the existing non-resumable commands, the caller must first stage a
SHA256SUMS inventory using an independently obtained SHA-256 pin. Both
commands revalidate that exact private inventory before any member transport.
The batch command iterates the inventory's deterministic page-range order.

The existing `wiki-source::public_dump_resume` implementation persists
bounded HTTP 206 range chunks in a private, generation/member-digest-scoped
checkpoint. It refuses linked/unsafe checkpoint paths, rejects inconsistent
range metadata, and rehashes **the entire assembled compressed member**
against the pinned SHA-256 before atomic no-overwrite publication. A failed
range preserves the completed prefix for the next invocation. Already
published complete members are rehashed and reused. Incomplete checkpoints
are **never** pack inputs or verified source members.

This wiring reuses the existing synthetic range, interruption, checksum
mismatch, inconsistent-total, and symlink tests in `wiki-source`, and the
CLI's independently pinned inventory and batch preflight tests. Production
range requests use curl's Content-Range write-out support (curl 7.84+), and
currently launch a new HTTPS request for each <=2 MiB chunk; this is a
correctness-first implementation, **not** an optimized large-dump downloader.

The independently supplied inventory digest is **not** publisher
authentication or completeness evidence. No live Wikimedia generation,
independent page census, revision-matched renderer, required offline media,
or desktop/Android end-to-end acceptance is qualified by this change.

## Checkpoint restart efficiency and crash recovery

At exact source SHA `1955190dd3705613aeb4e83d3232164ef66a097b`,
the resume loop avoids an expensive provisional copy of an incomplete prefix
on every restart. A previously synced partial continues at its byte offset.
An HTTP 416 at that offset (or reaching the caller's byte budget) triggers
a full checksum-verified publication attempt, never an assumption of success.
Corrupt completed prefixes are removed after the failed SHA-256 gate.

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40186867230 (passed)
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40186867268 (passed)

Synthetic regressions cover ordinary continuation, valid crash-after-final-
chunk recovery, and rejection/removal of a corrupted complete prefix. No
real Wikimedia transfer or device benchmark has been performed.
