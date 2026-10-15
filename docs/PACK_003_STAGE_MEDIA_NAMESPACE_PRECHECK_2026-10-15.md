# PACK-003 — Staged media-object namespace preflight (2026-10-15)

**Status:** Narrow hardening only. PACK-003 and SRC-003 remain unchecked.

## Defect

`check_declared_staged_media` rehashed all objects in a provisional staged pack,
then offered **every** manifest digest to the mandatory-visual checker, even
if the object path denoted an article, index or arbitrary non-media payload.
This allowed a declared visual reference to be satisfied by matching bytes in
`articles/` without any `media/` asset. The separate
`wiki-pack-format::media_coverage::audit_article_media` already restricts
eligible manifest objects to paths under `media/`.

## Correction

After `verify_manifest_directory` rehashes and validates the complete tree,
the staged-media preflight now considers only manifest entries whose path
starts with `media/` as candidates for required visuals. A deterministic test
constructs an immutable fixture tree containing only `articles/page.cbor`
with exactly the bytes and SHA-256 expected by a required diagram: preflight
must reject it. Adding a `media/diagram.svg` entry with the same bytes makes
the same requirement pass. Existing independent object verification, digest,
byte-bound and attribution requirements remain unchanged.

## Scope / limitations

This rejects a specific false-positive **declared media** coverage case. It
does not establish that the resource list is exhaustive, inspect rendered
HTML/CSS/fonts/MathML dependencies, identify true MIME formats, authenticate
Wikimedia publication/license provenance, or make the resulting pack safe for
offline reading. The current renderer's independent 25-page sample measured
316 live image references and 0 local references at prior `master`
`14b3cd0f3d01c68d1e082dd8d2c37f7526ee9aef`; it remains unqualified
for mandatory visuals.

## Qualification

Exact-head Rust and Platform CI verification is required after this change.
