# SRC-003: Digest-pinned public-dump checksum inventories (partial)

**Status:** Partial implementation, not an authenticated source importer or completed SRC-003.  
**Qualified source HEAD:** `dc597d58cbfe2fb1f43ddab30c461cc9f7232833`

The new `wiki-source::public_dump_pinned` API accepts the exact raw bytes of a SHA256SUMS inventory, a project, a current-content export date and an expected SHA-256 digest. It checks the digest and export identity, then exposes declared member records for staging. `stage_pinned_member` only stages a member from that inventory, checking streamed bytes against the member's SHA-256 before publishing. `verify_pinned_staged_export` rechecks every declared staged member.

The parser rejects malformed digest strings, invalid UTF-8, invalid/duplicate paths, wrong project or date, and modified inventory bytes. Synthetic tests also cover undeclared member indexes, corrupt staged data and a valid staging roundtrip.

## Qualification

- [Rust workspace CI 38258896472](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38258896472): passed at exact source HEAD.
- [Platform shells CI 38258896602](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38258896602): passed at exact source HEAD.

## Mandatory trust and acceptance boundaries

**The caller MUST obtain the expected inventory digest from an independently authenticated publisher source.** Computing the digest from the same untrusted download establishes no publisher trust. This API does not prove that an export is complete or even official. The staging tree must remain private against concurrent untrusted filesystem mutation; path preflight is not a race-free file-descriptor sandbox.

This is not a complete Wikimedia dump importer. XML parsing, revision/namespace normalization, template/Lua/math rendering, required visual assets, real source fidelity, pack export and offline desktop/Android acceptance remain unqualified. The canonical `docs/WIKIPEDIA_AI_READER_TODO.md` **SRC-003 checkbox must remain open**. No new publisher-trust label should be displayed based on this checksum check alone.
