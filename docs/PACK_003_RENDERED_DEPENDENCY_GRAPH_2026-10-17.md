# PACK-003 — Combined rendered HTML and CSS dependency discovery (partial)

Date: 2026-10-17
Status: Qualified **candidate discovery only**; PACK-003 remains unchecked.

## Implemented

The shared Rust `wiki-pack-builder::rendered_dependencies::inventory_rendered_dependencies` combines bounded HTML visual discovery (including inline CSS URLs and imports) with transitive external stylesheets supplied by the caller. It follows stylesheet links, preloads and inline imports; resolves relative URLs against each stylesheet; sorts and deduplicates typed URL candidates; rejects missing stylesheets/imports; and enforces a combined raw-reference budget even when URLs repeat.

Four regression tests cover transitive CSS images/fonts, missing imports, aggregate budgets, and unresolved dynamic surfaces. No network access or file reads occur.

## Limits

HTML/CSS inputs are caller supplied and unauthenticated. Both `css_and_font_dependencies_verified` and `required_media_verified` remain **false**. This is not proof of Wikimedia source completeness, safe CSS, verified bytes, attribution, local URL rewriting or offline readiness. Dynamic requests and actual desktop/Android airplane-mode behavior remain unverified.

## Exact-head qualification

Implementation SHA: `b397e8fdef3c95521f920758d362f51dc8d39288`.
Rust workspace CI `39010798675`: passed. Platform shells CI `39010798678`: passed. Both workflows ran at the exact implementation SHA. PACK-003 remains unchecked.

Next: map discovered URLs to authenticated revision-scoped media, verified objects and license notices, then qualify offline local-URI resolution and cross-platform rendering.
