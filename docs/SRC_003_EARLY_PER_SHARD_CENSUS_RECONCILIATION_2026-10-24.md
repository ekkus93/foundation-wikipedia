# SRC-003 — Early per-shard expected census reconciliation

**Date:** 2026-10-24  
**Status:** Scoped software implementation verified; SRC-003 remains open.

## Problem

The native `audit-public-dump` path previously verified every compressed XML
shard before comparing the observed namespace page IDs against the independent,
SHA-256-pinned expected census. If an early shard was missing a page or
contained a different valid page ID, the mismatch was not diagnosed until
every subsequent shard had been decompressed. On a large public dump this
could waste hours.

## Implementation

The native audit now reopens and rehashes the separately pinned expected
census after the existing digest, structural, and declared-range preflight,
and derives a SHA-256 receipt and count for each declared shard partition.
Receipts hash the exact sorted, canonical decimal page IDs followed by LF.
The receipts are bounded to one digest/count pair per pinned member and do
not materialize page IDs.

`PinnedObservedCensus` computes the same digest/count over the selected
namespace as each member is decoded and accepted. Nonselected namespaces
still participate in global page-ID ordering but do not contribute to
that namespace's receipt. An empty selected-namespace shard has the
standard SHA-256 empty-input digest and count zero.

After a native bzip2 member has completed its checksum, XML, revision
metadata, and shard-range checks, the native audit compares that member's
observed receipt with the expected receipt. A mismatch, including an
equal-count identity substitution, aborts before the next member opens.
Staging cleanup prevents publication of the provisional census. The
existing final independent whole-generation digest and exact-ID
reconciliation still runs before atomic publication.

This change is intentionally limited to the native audit path; the
generic semantic-event staging API retains its original contract.

## Automated evidence

- Implementation `ad80728359643207ec64a0dca694ede3b1162c89`;
  initial Rust formatting failure [40169740504](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40169740504).
- Formatting repair `1b4208b334e5cf3b42156c0d778909ada2381049`.
- Test import repair `5a62f06984c6206bab5588b24604691c8016ca9f`.
- **Qualified code SHA:** `d1e68e5c8af544a33af67cf28ebc478e0475ad41`.
- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40170007440):
  formatting, strict Clippy and Rust workspace tests.
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40170007457).

Synthetic tests exercise a real two-shard bzip2/XML native census,
a digest-pinned expected first-shard ID substitution with the same page
count (only the first source member opens), nonpublication on mismatch,
and a valid namespace-filtered member receipt including a shard with
zero matching-namespace pages. Existing valid native roundtrips and
the final independent audit continue to pass.

## Remaining qualification

Expected census and inventory SHA-256 pins are still supplied by the
operator; neither hash proves independent publisher origin, official
completion or compatible snapshot cutoff. Declared filename ranges are
not authoritative evidence of full source coverage. This path still
does not produce revision-matched Parsoid HTML, required offline media,
a verified installable pack, or real-device acceptance. **SRC-003 remains
unchecked.**
