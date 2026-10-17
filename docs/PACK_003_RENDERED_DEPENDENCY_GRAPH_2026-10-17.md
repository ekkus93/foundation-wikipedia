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

## Provisional content-addressed local URI plan (additional partial implementation)

The shared `wiki-pack-builder::rendered_local_urls::plan_verified_local_urls` now derives a deterministic original-URL to `wiki-asset://sha256/<64-lowercase-hex-digest>` mapping **only after** the complete rendered-object audit succeeds. Duplicate URLs and duplicate content digests remain deduplicated. The mapping is a logical identifier, never a filesystem path or authorization token; it performs no rewriting, IO, or network access. Regression tests cover transitive CSS/HTML candidates, same-content reuse, missing bindings, dynamic surfaces and tampered local bytes.

This is a necessary preparation for local URI resolution, **not a working renderer or permission to load the URI**. No desktop/Android scheme handler, owner authorization, HTML/CSS sanitization or rewrite has been qualified. The new plan explicitly reports both handler and rewritten-content qualification as false, and its object audit still reports `source_authenticated=false` and `offline_ready=false`. PACK-003 remains unchecked.

## Owner-scoped content-addressed reads (qualified partial implementation)

The shared Rust pack-builder now exposes `read_verified_pack_asset` as a **provisional pure-library read primitive**, not as a Tauri or Android WebView scheme handler. It requires a canonical `wiki-asset://sha256/<64-lowercase-hex>` URI present in the verified URL plan, confirms the requested digest belongs to the named **pack** owner in the SQLite media-ownership registry (a cache or pin alone is insufficient), and rehashes the stored object on *each* read, checking the registered length. No arbitrary filesystem path or network transport is accepted. Tests cover valid owner reads, missing/wrong pack ownership, noncanonical/traversal-shaped URIs, unplanned digests and modified bytes.

**Security boundary:** The URL plan and owner registry are caller-supplied local evidence. Neither authenticates the Wikimedia source, the pack manifest/signature, media notice, browser origin or MIME; ownership records alone do not prove installation or consent to render an unsafe media type. UI scheme authorization, trusted provenance, SVG/MathML sanitization, safe HTML/CSS rewriting and airplane-mode desktop/Android acceptance remain outstanding. No `handler_qualified` or `offline_ready` status is set to true. PACK-003 remains unchecked.

### Exact-head CI qualification

- Initial owner-scoped reader commit `108c9f4f4ae3d98496e8b067f713958ed76bf960`: Rust run `39014657657` failed **Format**, job `117344128859`, bounded log lines 299–359. This was authored formatting, not a Ralph or GitHub API failure.
- Formatter repair commit `474e7fee6fd76e8d1aa0c7e6c9e4e0262c2ff780`: [Rust workspace CI 39014728232](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39014728232) and [Platform shells CI 39014728214](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39014728214) both passed on the exact source SHA. Platform checks cover desktop frontend/Tauri and Android shell builds, **not** actual offline WebView asset loading.
