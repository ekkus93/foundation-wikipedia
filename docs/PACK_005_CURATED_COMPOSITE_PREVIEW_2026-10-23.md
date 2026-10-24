# PACK-005 — Revision-joined curated composite topic preview

**Date:** 2026-10-23  
**Scope:** Partial in-library selection preview, not production pack acceptance.

## Implementation

`wiki-pack-builder::curated_preview::preview_curated_topics` combines the
existing Mathematics, Physics and Computer Science curated seeds against a
caller-supplied batch of revision-joined Enterprise records. The function
canonicalizes requested IDs and roots, enforces one project and compatible
depth/page policies, supports explicit include/exclude page overrides, and
delegates to the existing pinned topic resolver and pre-download preview.

A composite's selection and revision receipts are invariant under topic
request ordering and duplicates. Wrong-edition records, unknown topic IDs
and excessive requested topic counts fail closed. This does not fetch or
authenticate a Wikimedia source, prove category coverage or build a pack.

## Qualification

- First code `51510b84a6bbd7235fc6c16dbd397de8914b8cff`
  failed rustfmt [40063481346](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063481346).
- First repair `fd4fe55e8d3a8190922dc5770279b39849d856ac`
  exposed additional rustfmt changes [40063516522](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063516522).
- Corrected exact code SHA `0d83347eb1aec5f4b267502b064c5d176d4a212b`
  passed [Rust workspace 40063563166](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063563166).
  [Platform shells 40063563178](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063563178)
  was in progress at documentation.
- Synthetic regressions cover deterministic composite selection, matching
  revision and definition receipts, explicit page overrides, single-topic
  selection, invalid topic requests and foreign-edition rejection.

## Remaining

Production desktop Pack Builder and CLI exposure, publisher-authenticated
complete category membership, measured fan-out/storage, required media,
pack build/signing and real-device verification remain outstanding.
**PACK-005 remains unchecked.**
