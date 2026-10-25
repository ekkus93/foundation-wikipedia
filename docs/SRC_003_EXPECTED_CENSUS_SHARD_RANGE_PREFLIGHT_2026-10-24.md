# SRC-003 — Expected census shard-range preflight

**Date:** 2026-10-24  
**Status:** Scoped software qualification; SRC-003 remains open.

## Failure mode

An expected page-ID census may have a valid SHA-256 digest, valid strict LF-decimal
encoding, and sorted unique IDs while being impossible to reconcile with the
page-ID partitions declared by the pinned current-content dump inventory.
Previously the native auditor could decompress every shard before discovering
this obvious mismatch. The direct post-ingestion census comparison could also
accept two matching files with impossible IDs because it only compared their
contents, not the pinned inventory's declared ranges.

## Implementation

`crates/wiki-source/src/public_dump_census_gate.rs` now validates that every
expected ID lies within a declared member range. The pinned inventory already
sorts disjoint ranges by numeric start, so the check advances monotonically
through the expected census and inventory in O(pages + shards) time and O(1)
additional memory. It does not expand sparse ranges into individual IDs.

The range preflight runs after expected SHA-256 and structural verification,
before any native member decoder opens, and again in
`audit_pinned_census_files` before comparing the observed and expected
censuses. A dedicated `ExpectedOutsideDeclaredRange(page_id)` diagnostic
identifies the first impossible page ID. Both paths preserve their existing
digest/metadata checks and atomic no-overwrite publication behavior.

## Regression and exact-head CI

Tests cover an expected ID in the gap between [1,100] and [201,300], an ID
beyond the final partition, and the direct post-ingestion audit with identical
but impossible expected and observed files. The native tests assert zero source
member opens and no published output for preflight failures. Existing valid
two-shard native bzip2/XML roundtrips continue to pass.

- Implementation SHA: `05b07715debbcbd684f23bbedb244804b41e2aed`
  (initial rustfmt failure).
- Corrected code SHA: `ba4d608cf01e978de857e41484af958a1ae52985`.
- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40166706120).
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40166706130).

## Scope limit

A declared shard range is a filename-level consistency assertion, not
evidence that every page in the range exists or that a published generation
is complete. The expected census and SHA256SUMS pins are caller-supplied and
remain unauthenticated. Publisher provenance, independent snapshot cutoff,
revision-matched HTML, mandatory media, pack installation, and physical
desktop/Android acceptance remain outstanding. SRC-003 stays unchecked.
