# SRC-003 — Early intra-shard namespace-count rejection

**Date:** 2026-10-25  
**Status:** Scoped software implementation verified; SRC-003 remains open.

## Problem

The native public-dump audit previously compared the observed namespace
page count and canonical page-ID digest to the independently pinned expected
census only after each bzip2/XML shard finished. A malformed or incomplete
generation with an unexpected surplus of selected-namespace pages could
continue decompressing a large shard even after the discrepancy became
irrefutable.

## Implementation

`PinnedObservedCensus::decode_native_bzip2_member_with_expected_count`
accepts a per-member expected selected-namespace page count derived from the
separately pinned expected census. Its shared decoder checks the next
selected-namespace count immediately after a complete validated page event,
before writing that page to provisional census output. The first excess
page poisons the pipeline and returns `MemberCensusMismatch` with the
member name, expected count and observed count.

The native `stage_and_audit_native_bzip2_census` path now supplies each
expected member count to the decoder. A nonselected namespace page does
not contribute to this limit, but still participates in page-ID ordering
and source semantic checks. The ordinary unbounded native staging path and
generic event-based path preserve their existing contracts.

This check is **one-sided**: a missing page or an equal-count page-ID
substitution still requires the full member's digest/count comparison.
The final independently pinned whole-generation census comparison remains
mandatory before atomic publication. A failed decode never exposes the
provisional output as a completed census.

## Automated evidence

- Initial source SHA: `a0bb3d27d7db49427e3376e8bf7521262632a8ba`.
  The initial Rust workflow failed rustfmt [40173862413](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40173862413).
- Formatting and empty-namespace test correction: `912a6f22ef78867bcf8071c47e9a6e7dd48a30ff`.
- **Qualified code SHA:** `b8c5786a9c79fe1d3764e3f53411ebd6aa9e01f4`.
- [Rust workspace 40174057488 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40174057488):
  formatting, strict Clippy, workspace tests and new native regressions.
- [Platform shells 40174057513 — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40174057513).

The unit regression checks that the first selected page exceeds an expected
count of zero, poisons the native decoder and prevents `finish` from
succeeding. It also checks that a page in another namespace does not
falsely exceed the selected-namespace count; an entirely empty selected
namespace still cannot be published by the standalone census writer.

The integrated two-shard regression supplies a valid pinned expected
census containing only a page from the second shard. The first shard's
unexpected selected page fails with expected count zero, observed count
one, opens no later shard, and publishes no census. Existing same-count
digest-mismatch tests and successful full roundtrips remain intact.

## Remaining qualification

This is fail-fast verification of local caller-pinned data. It does not
authenticate Wikimedia publication provenance or prove that the independently
supplied expected census is genuine, complete or snapshot-compatible.
Revision-matched rendered HTML, required offline images/diagrams/math,
pack activation, Android/desktop full-fidelity rendering and physical
airplane-mode acceptance remain unqualified. **Do not check SRC-003 complete.**
