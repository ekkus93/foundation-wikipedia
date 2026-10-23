# SRC-003 — Strict native redirect XML attributes (partial)

Date: 2026-10-23
Status: Synthetic native XML semantic parity; SRC-003 remains unchecked.

## Implementation

`wiki-source::public_dump_xml_events::XmlPageMetadataWriter` now rejects
any redirect-tag attribute other than `title`; it already rejected a
missing or duplicate title. Previously a syntactically valid
`<redirect title="Science" extra="ignored"/>` was silently accepted
despite a contradictory/unknown attribute. The Python diagnostic
`public_dump_xml_census.py` requires precisely the title attribute, so
this change aligns the portable native adapter with the diagnostic
schema. It does not change legitimate escaped title decoding.

## Qualification

Synthetic regressions feed XML to the incremental metadata writer in
1-, 3-, and 8192-byte chunks. They verify valid escaped redirect names
and rejection of extra attributes, unknown attributes before the title,
duplicate titles and an absent title.

Exact source SHA: `3b2485a2e3ba0a0f5d6f36a88efc9e0086f34ac8`.

- [Rust workspace CI 39993928915](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39993928915) — passed formatting, strict Clippy and tests.
- [Platform shells CI 39993928941](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39993928941) — running when documented; do not report a pass until confirmed.

## Limits

The parser is a conservative XML subset; successful metadata checks
are not proof of publisher authenticity or completeness. Matching
revision-aware render output and mandatory images/math/attribution,
canonical pack preparation, and physical desktop/Android acceptance
remain outstanding. **SRC-003 remains unchecked**.
