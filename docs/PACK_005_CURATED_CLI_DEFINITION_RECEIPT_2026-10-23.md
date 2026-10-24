# PACK-005 — Curated definition receipt in the CLI

**Date:** 2026-10-23  
**Status:** Qualified partial implementation.

## Change

`curated_definition_fingerprint` is now a public shared
`wiki-pack-builder::curated_preview` function. It canonicalizes curated
topic IDs, validates the effective 64-hex-character rules digest, and
fingerprints seed identities and editable fields using the same
domain-separated transcript as the revision-joined preview.

`wiki-pack preview-topic --json` now emits
`curated_definition_sha256` alongside `curated_topic_ids` for curated
requests; text previews display the same receipt. Custom-only previews
do not claim a curated definition receipt. This enables comparing local
curated seed definitions and overrides across previews without conflating
a generic category-root rule with a curated topic identity.

## Qualification

- First code SHA `09d92d8e004f4ae5e182b23274dcc17cdcca6c36`
  failed rustfmt ([40071364930](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40071364930)).
- Corrected SHA `75d29dce99f2991799884320b1f2ddc3ce7c77cc`.
- [Rust workspace 40071581278](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40071581278): passed.
- [Platform shells 40071581343](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40071581343): passed.
- Tests cover digest validity, topic ID order/dedup invariance and
  differing seed identities; existing CLI curated seed policy tests pass.

## Remaining

The CLI input remains a local, untrusted category graph. No independent
Wikimedia source authentication, complete topic fan-out, mandatory offline
media, signed pack or physical-device acceptance is claimed.
**PACK-005 remains unchecked.**
