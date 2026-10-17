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

## Verified local object binding (additional partial implementation)

The shared `wiki-pack-builder::rendered_objects::audit_rendered_objects` now requires an exact URL-to-content-digest/byte-length binding for every discovered resource, rejects missing/extra URLs and conflicting digest sizes, deduplicates shared objects and rehashes each local object through `wiki-store::media_inventory::verify_declared_media`. Unresolved dynamic surfaces, unsupported schemes and missing source attributes fail closed. A tampered local object or wrong declared size cannot pass this preflight.

The HTML/CSS discovery result also fingerprints the exact **caller-supplied CSS text** for every traversed stylesheet and transitive import. Object binding must match that SHA-256 and byte length, preventing a different local stylesheet from satisfying the candidate graph generated from another stylesheet's bytes.

These are necessary integrity checks, **not** source authenticity or offline-completeness certificates. The URL-to-object mapping itself is still a caller claim; license notices, CSS/HTML sanitization, source-to-byte authenticity, local URI rewriting, and real offline rendering remain outstanding. `source_authenticated` and `offline_ready` are always false; PACK-003 remains unchecked.

### Additional exact-head qualification

- Local-object binding implementation at `6dc32dd1eaf329c7938ddc20334d51879560c9af`: Rust workspace CI `39011194485` and Platform shells CI `39011194526` passed. Intermediate `d095c0acf8e88cfd8b888d11692035d2a9eb5792` failed Rust formatting (run `39011157719`, job `117333788245`, step 4), then was corrected.
- CSS text-to-object receipt binding at `3657f732afe2d0b6b6877306a902dab3ea65933a`: Rust workspace CI `39011314820` and Platform shells CI `39011314821` passed at the exact SHA.
