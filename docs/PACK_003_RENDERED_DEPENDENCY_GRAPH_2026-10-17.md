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

### Owner lookup optimization and recovery

`wiki-store::media_ownership::MediaRegistry::owned_digest_size` now performs a digest-specific SQL lookup with owner-kind and owner-ID constraints, rather than loading all registered media on each `read_verified_pack_asset` request. Tests cover pack-only authorization, cross-owner isolation, cache ownership not authorizing a pack and removal/revocation. This is an efficiency and fail-closed lookup improvement, **not** a browser scheme handler or offline-completeness certificate.

Recovered complete source SHA `4e99c038aa1407aa2958ad7a8e3495d31046423d` passed [Rust workspace CI 39015249765](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39015249765), [Platform shells CI 39015249724](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39015249724) and [Android Rust ABI CI 39015249722](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39015249722). Android codec probe `39015249701` was still running when documented. The initial formatting failure and accidental incomplete source reconstruction were repaired and are described in the canonical Ralph incident log. PACK-003 remains unchecked.

## Pack-bound complete local resource preflight (additional partial)

The shared Rust builder exposes `plan_pack_owned_local_urls`, which runs the complete discovered HTML/CSS graph-to-object byte audit and then requires *every unique digest* to belong to the selected pack's SQLite `OwnerKind::Pack` inventory at the declared byte length. Cache/pin ownership is insufficient; removed ownership, cross-pack requests and incomplete inventories fail closed. Duplicate URLs to one object do not multiply owner checks. No source attribution, signature, renderer-origin or HTML/CSS sanitization claim is made.

The first source SHA `fa7f4fcf2b64c7de943b1778de96f9cbe6fc2b61` failed Rust formatting (run `39015985192`, job `117348072181`). Corrected source `dbc3a429026a6f0e50199cb2df249e3bfee06a84` passed [Rust workspace 39016032674](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39016032674) and [Platform shells 39016032722](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39016032722). PACK-003 remains unchecked pending authenticated source/manifest, complete rendering resources and real desktop/Android airplane-mode qualification.

## Raster MIME and signature gates (qualified partial)

Added `wiki-store::media_ownership::MediaRegistry::owned_digest_mime` to resolve a unique MIME claim for one pack-owned digest without trusting cache/pin ownership. If multiple distinct MIME labels exist for the same stored digest, the result fails closed rather than choosing an arbitrary HTTP content type.

The shared Rust builder adds `read_pack_raster_asset`, which uses the existing pack-owner and SHA-256 read verification before allowing a candidate PNG/JPEG/GIF/WebP MIME whose leading bytes match that media type. SVG, MathML, HTML, CSS, fonts and unknown MIME types are **not** accepted by this provisional raster path. This is a signature sniff, **not** full image decoding, content sanitization or a WebView protocol handler.

Implementation commit `052cf7e61e0546dafbb0ed785ec8665c3c258058` initially failed rustfmt (run `39016434881`, job `117349402511`); repaired source `9673611ee373bb26ecb9b034a2e5e29cf160f25c` passed [Rust CI 39016581419](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39016581419), [Platform CI 39016581541](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39016581541) and [Android Rust ABI CI 39016581526](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39016581526). Android API 35 codec probe `39016581442` (job `117349829807`) subsequently **passed** at the same source SHA on 2026-10-17 08:30:54 UTC; it validates the record codec probe, not offline asset rendering. PACK-003 remains unchecked.

## Full WebP RIFF chunk-boundary preflight (qualified partial)

The optional `webp_riff_preflight` wrapper now validates the outer declared RIFF size and bounds **every** subsequent chunk header, payload and padding position, not only the initial `VP8 `/`VP8L`/`VP8X` chunk. A finite 4,096-chunk ceiling fails closed on extreme input. Regression cases include a permitted trailing metadata chunk and two malformed trailing structures. The underlying local asset read still requires pack ownership, SHA-256 object verification and MIME signature checks.

Exact source commit `239778fc3b3909110a41099a18799e7dbcba5e5f`: [Rust workspace run 39018828398](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39018828398) **passed**; [Platform shells run 39018828396](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39018828396) **passed**. The preceding two-line rustfmt repair at `2110843acf6a46ebea8d5bee8f8ef985ce4ea002` passed Rust run `39018559750` and Platform run `39018559730`.

This is not a complete WebP parser/decoder, source-authenticity guarantee, media sanitizer, browser URL handler or offline rendering proof. PACK-003 remains unchecked.

## Registered attribution notices returned alongside verified raster bytes (partial)

The storage registry now exposes `MediaRegistry::owned_digest_notices` for a pack-owned digest. It returns stable, individually stored MIME, source URL, creator and license notices (including multiple creators for deduplicated bytes), enforces the requested owner and validates persisted notices. This does not prove the truth of any notice or tie notice provenance to a particular publisher revision; shared digests may carry notices registered from different sources.

`VerifiedRasterAsset` in the shared pack builder now includes those notices along with pack-owned, SHA-256-verified raster bytes and the unique declared MIME. The reader's existing fail-closed cross-pack restrictions and mismatch checks remain in force. Regression tests cover two notices on one asset and registry revocation behavior.

Exact source SHA `7e950e7041701191691e7818854f36406b91e596`: [Rust CI 39019224006](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39019224006), [Platform CI 39019224111](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39019224111) and [Android Rust ABI 39019224062](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39019224062) passed; [Android codec probe 39019224066](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39019224066) still running at last check. Integration SHA `fac3aef76ed507fba273e5b54266852647ecaba0`: [Rust CI 39019411046](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39019411046) and [Platform CI 39019411038](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39019411038) passed.

Required media acquisition, authenticated source/revision metadata, robust sanitizers, HTML/CSS rewriting, actual browser resource handlers, attribution UI and representative airplane-mode acceptance remain incomplete. PACK-003 is still unchecked.

## Source-matched media notices and attribution text validation (partial)

The source-scoped `read_pack_raster_asset_for_source` resolves a URL only if it was included in the validated resource plan, then verifies pack ownership, actual bytes, MIME, and returns the subset of notices registered with that exact URL. A missing URL binding or an asset lacking a registered notice for the particular URL fails closed. Different source URLs may point to identical bytes; caller must not assume all digest-wide attribution applies to any one source. Exact repaired SHA `4020bf2f669b2bee7b8a0a5f1971b17baed6a424` passed [Rust CI 39020196554](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39020196554) and [Platform CI 39020196552](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39020196552). Initial SHA `bb959766ff5c492fb6540688cd5c4e144d65b042` failed a rustfmt-only gate and was corrected.

`MediaNotice` registration now rejects creator/license control characters and Unicode bidirectional overrides/isolates (tests for both fields), while retaining ordinary international display text. Reconciled source commit `aecaecf0f69fd52d790c309318b3e086730d02a2` passed [Rust CI 39020370594](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39020370594), [Platform CI 39020370608](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39020370608) and [Android ARM64 ABI 39020370616](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39020370616); [Android codec probe 39020370607](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39020370607) remained in progress at last check.

This is not upstream-authenticated notice provenance, HTML escaping/sanitization at presentation, actual attribution UI, trusted manifest verification or offline end-to-end acceptance. Parent tasks remain unchecked.
