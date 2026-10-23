# SRC-003: XML namespace attribute-value normalization

**Date:** 2026-10-23  
**Status:** Scoped streaming XML namespace semantics; SRC-003 remains unchecked.

## Behavior

Under XML 1.0 attribute-value normalization, literal tab, line feed,
carriage return and CRLF inside a quoted attribute value are normalized
to spaces before character-reference expansion. Numeric character
references such as `&#xA;` are decoded *after* the literal-whitespace
normalization and retain the referenced character. Comparing raw
attribute values or expanding references without first normalizing can
misidentify two prefix namespace URIs as different.

The native `XmlPageMetadataWriter` now normalizes namespace declaration
values using XML 1.0 literal-whitespace rules before resolving entities
and evaluating reserved/default namespace identities and expanded
attribute-name collisions. CRLF is treated as one logical line ending.
This remains a bounded diagnostic metadata reader rather than a complete
general-purpose XML parser.

## Evidence

- Source commit: `1549ce9bd5918efb0f27bc7cf3f7af7b1a1561f9`.
- Regression cases create two aliased namespace prefixes using raw CRLF
  or tab versus an ordinary space, then require rejection of repeated
  expanded attribute names. Tests run at chunk sizes 1, 2, 7 and 8192.
- Exact code SHA [Rust workspace CI 40023142919](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40023142919).
- Exact code SHA [Platform shells CI 40023142952](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40023142952).

## Remaining acceptance requirements

Synthetic XML normalization tests are not upstream Wikimedia publisher
authentication or evidence that the census independently covers an
authentic matching generation. They do not produce revision-matched
Parsoid/MediaWiki HTML, required offline images/diagrams/math/captions,
license attribution, or real-device acceptance. Keep SRC-003 unchecked.
