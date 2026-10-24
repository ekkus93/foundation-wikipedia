# PACK-005 — Curated definition fingerprint

**Date:** 2026-10-23  
**Scope:** Partial reproducibility metadata for curated topic previews.

## Implementation

`CuratedTopicSelection` now includes `curated_definition_sha256`, a
domain-separated SHA-256 transcript over canonical sorted topic identifiers,
the editable curated seed definition fields (ID, display label, edition,
category roots, depth limit and page limit), and the effective generic rule
definition digest. Length-delimited strings and explicit counts prevent
ambiguous concatenation. This is a stable local definition receipt, not a
signature, a Wikimedia source authority proof or a pack manifest.

The existing revision-pinned selection hash remains separate. The new
fingerprint changes when curated topic identity or explicit include/exclude
overrides change, even if a generic root list alone could appear identical.

## Qualification

- Initial source SHA `3517cbc42624f20909dc4f64f777f6e8f45afa44` failed rustfmt
  ([40070701222](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40070701222)).
- Corrected source SHA `1574cf2ec8eb7b9097e923a3ba017ea7acd729f0`.
- [Rust workspace 40070739028](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40070739028): passed.
- [Platform shells 40070739007](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40070739007): passed.
- Regression covers order/duplicate invariance, 64-digit digest output,
  distinct curated topic identities and override-sensitive receipts.

## Still required

Authenticated Wikimedia category completeness and actual fan-out
measurements, complete deterministic topic packs with mandatory media,
publisher trust, desktop/Android integration and physical acceptance remain
outstanding. **PACK-005 remains unchecked.**
