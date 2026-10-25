# SRC-003 — Streamed intra-shard page-ID comparison

**Date:** 2026-10-25  
**Status:** Scoped implementation and synthetic regression qualified; SRC-003 remains incomplete.

## Rationale

The native public-dump auditor already compared an expected count and
SHA-256 receipt for each inventory-pinned shard after its full bzip2/XML
decode. It could also reject selected-namespace page-count overflows
during decode. A substituted page ID with the same selected-namespace
page count, however, was only rejected when that whole shard ended.
For a large Wikimedia shard this could defer an unavoidable rejection
through expensive decompression.

## Implementation

`PinnedObservedCensus::decode_native_bzip2_member_with_expected_ids`
accepts a strictly streamed, externally pinned page-ID iterator and the
already established member-specific count. The native XML callback
compares the next decoded selected-namespace page identity immediately
after completing its pinned page event. A distinct expected ID fails as
`MemberPageIdMismatch` with member name and both IDs; exhausted or
unreadable expected streams fail closed as `ExpectedCensusChanged`.

The shared native audit reopens and SHA-256-verifies the expected census
through the regular-file safety boundary, streams IDs in shard order,
and ensures the cursor has no remaining records after the last shard.
Nonselected namespaces never advance the iterator. The existing early
count guard runs before this comparison, so unexpected extra pages
cannot consume the next shard's expected page.

The old general native decoder and semantic-event interfaces remain
compatible. A full compressed-member integrity check, complete-member
selected-namespace digest and count receipt, and final whole-generation
audit remain mandatory, even when every streamed ID matches.

Memory is bounded to the page-ID reader's fixed parser record and
buffer; the expected census is never materialized as a complete vector.
The existing small vector of per-member count/digest receipts remains.

## Automated qualification

- **Qualified source SHA:** `191c09932db548f05c1ccc5f564f2685f46482f7`.
- [Rust workspace CI 40176385614 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40176385614): formatting, strict Clippy and workspace tests.
- [Platform shells CI 40176385654 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40176385654).

The new native compressed XML fixture contains an unrelated namespace
page followed by two selected-namespace pages. It verifies a matching
stream, an equal-count substitution rejected at the actual page, an
exhausted expected iterator, and permanent failure/nonpublication.
The independent two-shard audit tests retain exact-ID mismatch and
source-digest rejection paths as separate cases. Initial commits
`bd7746b3931308dc116349b4428123ac3dd55de5` and
`e86e7a1c20cfe372498e7ea197a58699d004816d` needed formatting,
Clippy and fixture/test-expectation repairs. The qualified head is the
corrected code revision; no earlier failing commit is acceptance evidence.

## Limits and remaining acceptance

Both inventory and expected census are supplied and integrity-pinned by
the caller. Matching them does not establish independent publication
authority, compatible snapshot cutoff, source completeness or an
authentic Wikimedia generation. Official provenance authentication,
revision-matched rendering, required offline media, full snapshot/pack
activation, desktop/Android integration and physical acceptance remain
unqualified. Leave **SRC-003 unchecked**.
