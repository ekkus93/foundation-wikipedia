# SRC-003: Certificate-verified compressed-member staging (2026-10-25)

Status: partial software qualification. SRC-003 remains unchecked.

## CLI workflow

1. Obtain an independently authenticated SHA-256 for the official Wikimedia
   current-content SHA256SUMS generation.
2. Create a private mode-0700 source directory and run
   `wiki-pack fetch-public-inventory PROJECT GENERATION INVENTORY_SHA256 SOURCE_DIR`.
3. Run `wiki-pack fetch-public-member PROJECT GENERATION INVENTORY_SHA256 SOURCE_DIR MEMBER_INDEX MAX_COMPRESSED_BYTES`
   for each declared checksum-inventory index (zero-based).

The second command opens and digest-verifies the private staged SHA256SUMS,
rejects wrong project/generation/checksum or out-of-range member indexes,
then constructs the exact official HTTPS URL bound to that inventory entry.

The host-only curl subprocess uses certificate verification, no user curlrc
settings or redirects, HTTPS protocol restriction and fixed time limits.
Compressed response bytes are streamed into the existing private,
checksum-verified `stage_pinned_member` boundary. Crucially, stdout EOF
does not count as successful completion until curl has exited successfully
and supplied an explicit HTTP 200 status. HTTP failures, incomplete input,
oversized bytes and mismatched member SHA-256 cannot publish staged output.
Existing valid member re-staging follows the existing idempotent,
digest-checked, no-overwrite semantics.

No arbitrary member URL is accepted. No article is rendered, pack activated
or user state changed. Android needs its own platform HTTP adapter.

## Automated evidence

Qualified implementation head:
`e095ceaf4671269f0dd814a185dbef7996725dff`

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40180209706 (passed)
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40180209778 (passed)

Synthetic tests cover a successful staged response, redirect and nonzero
exit after an otherwise valid body, missing success marker, wrong checksum,
byte budget excess, invalid inventory member index, invalid locally staged
inventory, wrong project/date and permanent nonpublication after errors.
These tests use fake curl and fixture bytes, not a live Wikimedia export.

## Remaining qualification

A checksum inventory obtained over TLS with a separately supplied digest
still needs independent Wikimedia publisher provenance, snapshot cutoff
and generation-completeness evidence. This first implementation downloads
one member at a time, without resume/range/retry support or real source
release acceptance. The source still requires independently validated
page census, revision-matched wikitext/Parsoid renderer, mandatory offline
media, snapshot/pack activation, offline search and real desktop/Android
acceptance. **Leave SRC-003 unchecked.**
