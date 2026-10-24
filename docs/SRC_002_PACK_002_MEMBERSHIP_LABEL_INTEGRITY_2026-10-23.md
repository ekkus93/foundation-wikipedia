# SRC-002 / PACK-002 — Membership label integrity at source and resolver boundaries

**Date:** 2026-10-23  
**Scope:** Partial source and topic-resolver input validation; not complete source or pack qualification.

## Implementation

- `wiki-source::enterprise` rejects embedded Unicode line/paragraph separators
  U+2028/U+2029, U+200B zero-width space, U+2060 word joiner, U+FEFF BOM
  and deprecated U+206A–U+206F bidirectional controls in category/redirect
  membership labels. Existing directional formatting checks remain.
- `wiki-pack-builder::selection` applies the same U+2028/U+2029 and
  U+206A–U+206F exclusions to roots, child links and all supplied graph
  node labels, including unselected categories.
- Public typed `RegularArticleCompanion` values bypass JSON decoding;
  direct `join_enterprise_article` and batch joins now revalidate all
  category/redirect labels and reject duplicate memberships.
- Persian ZWNJ and Indic ZWJ category spellings remain valid.

## Regression coverage

- Enterprise parser: forged category and redirect labels containing
  hidden controls, Unicode paragraph/line separators and overlong labels.
- Direct and batch typed joins: invalid labels and duplicate categories/
  redirects cannot bypass parser checks.
- Topic resolver: invalid root, child and unselected graph labels are
  rejected; valid multilingual joiner spellings remain accepted.

**Code SHA:** `389c9e8f51b7d59403e005b7422792da113c6034`.

**Exact-head CI at documentation:**
- [Rust workspace #40059024029](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40059024029) — running.
- [Platform shells #40059023993](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40059023993) — running.

Earlier implementation heads failed Rust formatting, which was repaired
on `master`. Confirm terminal CI for the corrected code SHA before
treating this as a qualified slice.

## Outstanding

The changes do not authenticate the publisher, prove category/redirect
coverage, implement revision-matched rendered article fidelity, qualify
mandatory offline media or establish real-device acceptance. SRC-002 and
PACK-002 remain unchecked.
