# PACK-002: Raw topic-graph category label validation

**Date:** 2026-10-23  
**Status:** Partial topic resolver hardening; PACK-002 remains unchecked.

## Change

The public `wiki-pack-builder::selection::resolve_with_redirects` path
now validates category labels before traversal. All requested roots, all
graph keys, and every child edge are checked, including unvisited nodes.
This matters because raw caller-supplied `Rules` and `Category` DTOs
can bypass the Enterprise parser and its own category-label checks.

The policy rejects empty, leading/trailing whitespace, >4,096 UTF-8-byte,
XML/control, directional-spoofing, zero-width-space, word-joiner and BOM
labels. It preserves legitimate Persian ZWNJ and Indic ZWJ orthography.
A new typed `ResolveError::InvalidCategory` identifies rejected labels.
The pinned resolver inherits this guard before generating a selection
receipt, so an invalid unselected category cannot be silently fingerprinted.

## Qualification

- Initial code SHA `349c7deaac2d7c36d4c1c313bb7938484ae1284d`
  failed [Rust workspace 40042747909](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40042747909)
  only at rustfmt.
- Corrected code SHA `6a2599702710b8a21b2c2f4f6e9803ede5a5938b`
  passed [Rust workspace 40042806330](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40042806330)
  (format, strict Clippy and workspace tests) and
  [Platform shells 40042806360](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40042806360).
- Regression covers invalid root labels, malformed unselected graph keys,
  invalid child labels, >4,096-byte labels, and valid multilingual labels.
- No new branch or pull request; code and qualification were written
  directly to `master`.

## Remaining

No upstream publisher authenticity, complete category-graph census,
revision-matched rendered articles, full pack builder, required offline
media, desktop/Android installation, or physical-device acceptance was
established by this change. Do not mark PACK-002 complete.
