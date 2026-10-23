# SRC-003: XML redirect title attribute-value normalization

**Date:** 2026-10-23  
**Status:** Scoped native public-dump XML semantic fix; SRC-003 remains unchecked.

## Defect and fix

XML 1.0 normalizes literal tab, line feed, carriage return, and CRLF
inside a quoted attribute value to spaces **before** resolving entity
references. The native public-dump metadata adapter already performed
this normalization for namespace declarations, but the `redirect title`
attribute was decoded directly. Consequently, a legitimate title
attribute containing a literal tab or line ending was rejected as a
control character, rather than being interpreted as the XML-normalized
title. This also made provisional redirect identities differ from the
values that a conforming XML parser would produce.

`XmlPageMetadataWriter::redirect_title` now uses the same
`decode_attribute_value` normalization as namespace declarations.
The normalizer collapses CRLF into one space, converts other literal XML
attribute whitespace into spaces, and expands entities afterwards.
A numeric reference such as `&#x9;` therefore retains its tab and is
correctly rejected by the conservative redirect-title validator.

## Tests and exact-head evidence

- Tests exercise literal tab, LF, CR, and CRLF in redirect titles and
  verify that all yield the identical provisional `Science Math`
  redirect target, at write chunk sizes 1, 2, 7, and 8,192 bytes.
- A negative test confirms `Science&#x9;Math` is not silently normalized
  to an allowed navigation title.
- Initial implementation SHA: `098dffc63c2ca3b2c3832987630233e6fd3296ff`.
  [Rust workspace 40027910150](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40027910150)
  failed only the formatting step (array layout).
- Corrected code SHA: `a8b004a1064063ca818552165056c5f3c59d3489`.
  [Rust workspace 40028375326](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40028375326)
  passed rustfmt, strict Clippy, and workspace tests.
- [Platform shells 40028375299](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40028375299)
  was still in progress when this note was prepared; Android shell and
  desktop frontend jobs had passed, desktop Tauri remained running.

## Acceptance boundary

This only fixes redirect-title identity extraction from syntactically
validated public-dump XML. It does **not** independently authenticate a
Wikimedia publisher or generation, prove shard census completeness,
render revision-matched article bodies, include required offline
images/diagrams/math/captions/licenses, or qualify real desktop/Android
offline use. Keep SRC-003 unchecked.
