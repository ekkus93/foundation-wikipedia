# SRC-002 / PACK-002 — Typed joined graph identity and uniqueness

**Date:** 2026-10-23  
**Scope:** Partial validation of caller-constructed Enterprise joined records.

## Implementation

The category graph builder now imports the public Enterprise source adapter's
membership-label validator rather than maintaining a second, divergent Unicode
list. This rejects Unicode line/paragraph separators and deprecated bidi
formatting controls (including U+206A–U+206F) on category and redirect
identities even when a caller directly constructs a `JoinedEnterpriseArticle`.

Every joined record is checked for duplicate category memberships *after*
canonical `Category:` normalization. Duplicate redirect titles within a
record or across distinct records now fail closed instead of being silently
collapsed into the unresolved redirect-title set. Mainspace titles are
validated, and malformed category-page titles continue to return the
category-specific error. Valid Persian and Indic joiners remain supported.

## Evidence

- Initial implementation `fc4174acc525c4c3e890728e40d8619576a08e9e`
  failed Rust formatting [40063231118](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063231118).
- Formatting repair `b8598717d09d53acd042246b4a0bcc0e914cf46f`
  reached Rust tests, exposing a changed error classification in an existing
  category-title regression [40063273597](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063273597).
- Corrected exact code SHA `c5b2b4194060e5067ede9d700d60efbad4f5b415`
  passed [Rust workspace 40063381351](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063381351)
  (format, Clippy, tests) and
  [Platform shells 40063381349](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40063381349).
- Regressions cover semantically duplicated categories, repeated and
  cross-page redirect aliases, forbidden Unicode separators/bidi controls,
  malformed article titles and legitimate multilingual joiners.

## Still required

These checks establish neither official Wikimedia publisher authority nor
complete source category/redirect inventories. They do not establish
revision-matched rendered media, complete installable packs or physical
desktop/Android acceptance. **SRC-002 and PACK-002 remain unchecked.**
