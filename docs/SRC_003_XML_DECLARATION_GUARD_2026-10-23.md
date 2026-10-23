# SRC-003 — XML declaration/encoding consistency (2026-10-23)

## Scope

The portable public-dump decoder validates UTF-8 and XML 1.0 character legality. Before this change, its XML markup-envelope guard ignored XML declaration pseudoattributes and could accept a file declaring XML 1.1 or UTF-16/ISO-8859-1 despite interpreting the decoded stream as UTF-8 XML 1.0.

`XmlNestingGuard::valid_xml_declaration` now checks the declaration's ordered pseudoattributes: required `version='1.0'`, optional `encoding='UTF-8'` (case-insensitive), and optional `standalone='yes|no'`. Duplicate/unknown/missing fields, unsupported encodings/versions and reserved case-variant `<?XML ...?>` targets fail closed. Other ordinary prolog processing instructions, such as `xml-stylesheet`, remain accepted. This is deliberately narrower than a full XML parser.

Tests exercise valid declarations, encoding/version mismatches, duplicate and missing version, invalid standalone and case-variant reserved targets with input chunked into single bytes or pairs.

## Evidence

- First source SHA `6eaab84c4ac4e144404e45aace3d6e5110d375b0` failed rustfmt (Rust workspace run 39966808872).
- Corrected source SHA `e07fe556a0a0787c3acdc9926724611377cd6912` passed Rust workspace CI run 39966898568, including format, strict Clippy and tests.
- Platform shells run 39966898420 was still in progress when this note was prepared.

## Remaining qualification

The current XML envelope guard is not a conforming XML 1.0 parser and does not extract validated page/revision semantic events. XML declaration placement, namespaces and other full parser semantics still require proper parser integration. No independent upstream authentication, member census, revision-matched HTML, mandatory offline media or desktop/Android acceptance is established. **SRC-003 remains unchecked.**
