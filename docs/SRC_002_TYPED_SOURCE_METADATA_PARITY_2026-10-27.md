# SRC-002 — Typed Enterprise source metadata parity

**Date:** 2026-10-27 (America/Los_Angeles)  
**Scope:** Partial, source-adapter boundary; not a complete SRC-002 acceptance  
**Code SHA:** `980758f8c411934e1c7a76d4109bee67a87eb152`

## Failure mode

The Enterprise JSON parsers enforced nonempty required metadata, but public Rust `StructuredArticle` and `RegularArticleCompanion` values could be constructed without passing through JSON. `join_enterprise_article` previously checked page, revision, generation, namespace, language equality and category/redirect labels but could return a successful join with an empty or spoofed title, whitespace-only revision timestamp, blank rendered HTML, invalid Wikidata Q-ID or whitespace-only optional description. Matching two equally malformed typed DTOs is not proof of valid source data.

## Implementation

In `crates/wiki-source/src/enterprise.rs`:

- Both parsed JSON article identities and public typed joins now apply `valid_membership_label` to canonical article names. This rejects empty/whitespace-padded, oversized, control, bidi-format and invisible-spoofed names while preserving legitimate non-Latin text and script joiners.
- The typed join now rechecks required revision timestamp text, nonempty language identifiers, required rendered HTML, optional structured Wikidata `Q[0-9]+` identity and optional nonempty description. Individual required text fields cannot exceed the parser's 16 MiB record ceiling.
- Existing page/revision/generation equality, namespace/language equality, membership duplicate checks and batch tombstone/duplicate handling remain unchanged. No publisher authenticity is inferred from these validations.

## Regression and CI

The new `typed_join_rejects_missing_or_spoofed_source_fields_before_publication` test checks valid parsed single/batch joins; empty, padded, control and bidi-formatted names; invalid timestamp text in either input; empty language; blank HTML; malformed Wikidata IDs; blank optional descriptions; and a JSON-escaped spoofed canonical name. Both direct and batch joins reject malformed names. This is synthetic DTO validation, not a live Wikimedia source audit.

- Initial source SHA `78219137ac40e9a0ae16242cfe1c2040eef8f851` failed Rust format-only CI [40628495327](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40628495327).
- Corrected source SHA `980758f8c411934e1c7a76d4109bee67a87eb152` passed [Rust workspace 40628815287](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40628815287) (format, strict Clippy and tests) and [Platform shells 40628815304](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40628815304).

## Remaining qualification

**SRC-002 remains unchecked.** These guards do not independently authenticate a complete Wikimedia source generation, establish category/redirect census completeness, verify every rendered visual against its exact revision, sanitize HTML for a WebView, produce a complete offline-ready pack, or prove desktop/Android acceptance. More complete typed nested-block validation and publisher-backed source joins remain separate work.
