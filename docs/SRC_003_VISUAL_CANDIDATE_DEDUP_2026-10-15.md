# SRC-003 — cross-page visual URL deduplication and candidate fingerprint (2026-10-15)

The diagnostic candidate list now has a domain-separated, deterministic
`inventory_sha256` over a canonical sorted JSON array of (kind, URL)
entries. Duplicate/order-only rendered references retain the same digest,
while a different URL changes it. This digest is **not** source authenticity,
a verified media-object hash, a license receipt, or proof of exact template
dependency revisions.

The 25-page renderer aggregate now distinguishes total candidate references
(counted once for each page/kind) from distinct (kind, URL) candidates across
pages. Deduplication is observational; independently referenced thumbnail
sizes and URL variants are separate candidates until a later media downloader
verifies actual bytes/content identity. All results are bounded by the
per-page inventory caps, remain in retained CI artifacts, and cannot produce
an offline-ready pack without complete CSS/font discovery, Commons metadata,
authenticated downloads and installed-object verification.

**Qualification:** exact-head Rust/Python, Platform and renderer CI must pass.
Canonical SRC-003/PACK-003 checkboxes remain unchecked.

## Qualified source and CI coverage

The candidate-fingerprint/deduplication implementation landed directly on
`master` at `c28a562b9c5f8b3dea32c358b1feadb38be36a2f` via Ralph
operation `75534212a9794614808a7e7b2a5ff3da`. A narrow follow-up at
`cfad4f7e1f932b80b82e502b06eead04578dbf61`, operation
`14faba1893044f25a6049c56daa212bd`, clarified the NUL-delimited
fingerprint transcript and added a deterministic expected-digest fixture.
No initial source/test CI failure was required for that correction.

The dump renderer workflow originally omitted the newly added visual
inventory Python module and its tests from the push path filters/unit test
command. At exact head `3ad2bf0c3562e24ef0167ccebd118eead5ba1e95`,
operation `9d04b8a6e9e14d0e864e6db83340226b`, those dependencies were
added to `.github/workflows/renderer-dump-qualification.yml`.

All final exact-head workflows **passed**:
[Rust 38826792734](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38826792734),
[Platform 38826792795](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38826792795),
and [Renderer dump 38826792779](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38826792779).
Renderer job `116736008864` shows the 25-page diagnostic emitted **533
per-page live URL-kind candidates versus 25 local candidates**, but only
**441 unique live candidates versus 3 unique local candidates** when
deduplicated by URL and resource kind across articles. The 316-vs-zero
image-reference discrepancy and 23 pages with inline CSS persist.
URL-kind uniqueness cannot substitute for byte-level deduplication or
license/creator validation, and none of these metrics qualify offline-ready
articles or packs.

## Ambiguous data-URL responsive image protection

Diagnostic `srcset` splitting deliberately is not a full WHATWG grammar
implementation. A `data:` candidate contains a comma separating metadata
from its payload, which must not be misinterpreted as a second relative
remote-media filename. The candidate inventory now fails closed when it sees
a `data:` candidate in `img` or `picture/source` responsive variants,
with a deterministic regression. This protects the candidate list from a
false external-resource identity. It does not make the remaining simplified
`srcset` grammar suitable for certifying mandatory offline media.

## Exact-head qualification of ambiguous data-URI protection

The fail-closed data-URI-in-srcset regression is committed directly to master
at `ba93bd1f03fe155ca69742852a6ed866c21abc64` by Ralph Bridge operation
`dff2ca61449a4cdeb53f7475aa746f33`, `accepted=true`,
`write_reconciled=false`. All exact-head workflows passed:
[Rust 38827278461](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38827278461),
[Platform 38827278501](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38827278501),
and [Renderer 38827278454](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38827278454).
The update rejects malformed/unsupported responsive-media candidates rather
than falsely treating them as verified local assets. It does not establish
complete srcset or CSS parsing, Commons media provenance, verified offline
resource bytes, or any release acceptance.

## Observed-reference budget — qualified 2026-10-16 UTC

At exact master SHA `0bad2d152e81d6b33daa6c8172460524c396a5b6`,
the visual inventory now counts all references before deduplication, including
repeated URLs, missing sources, unsupported schemes and SVG fragments. It
exposes `observed_references` separately from the distinct candidate count.
Synthetic regression tests cover duplicate URLs, missing/unsupported sources,
SVG fragments and repeated responsive variants at a one-reference budget.

Ralph operation `98474e926fce41519e21eb42bdc5d90c` was accepted and verified.
Exact-head [Rust 38830731817](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38830731817),
[Platform 38830731900](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38830731900)
and [Renderer 38830731899](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38830731899)
all passed. This is a bounded diagnostic improvement, not a complete media
inventory, authenticated source, verified attribution or offline-ready pack.
SRC-003 and PACK-003 remain unchecked.

## Empty srcset candidates — qualified 2026-10-16 UTC

At exact source SHA `1c524975b6833632b1a837027ac560a64e564ba2`, the diagnostic inventory now rejects leading, consecutive or trailing empty comma-delimited `srcset` entries rather than silently omitting them. Synthetic regressions cover all three positions. Ralph Bridge operation `462cd56f9f914422b4707e5d3e411646` accepted and verified the direct-master source change. Exact-head [Rust 38836350370](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38836350370), [Platform 38836350351](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38836350351) and [Renderer 38836350359](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38836350359) all passed. The simplified `srcset` parser is not yet a production-complete media resolver, and SRC-003/PACK-003 remain unchecked.

## Responsive descriptor fail-closed diagnostics — 2026-10-16 UTC

The diagnostic inventory now validates positive width/density descriptor syntax and rejects malformed, duplicate, and mixed descriptor families before treating responsive URLs as candidates. This is a deliberately conservative diagnostic policy, not a full WHATWG parser or proof of complete offline media. Implementation/fix commits: `839e6c09b91e9aec4b8d2d2f6dc165f1aa7c7d81`, `be39331de0243c04550ea3ea302b76cc9b45aeb9`, `df220fa071cbaa483fc0393673839c90e14fee59`, `32b0f4cbe36e086ca4c0da8f2ee80b9e967f9cd1`. The initial regex and fixture regressions were diagnosed from exact-head Renderer CI and corrected; final Rust/Platform/Renderer qualification on `32b0f4cbe36e086ca4c0da8f2ee80b9e967f9cd1` is pending. SRC-003 and PACK-003 remain unchecked.

## Inline CSS diagnostic surfaces — qualified 2026-10-16 UTC

At exact source SHA `bcdcccd622b0c7199988a85932dabf1ead3f68df`, the inventory counts both `<style>` elements and HTML `style` attributes and enforces separate per-page budgets, so a page cannot hide arbitrarily many unparsed inline CSS surfaces from diagnostics. The flags still explicitly report `css_and_font_dependencies_verified=false` and `required_media_verified=false`. Synthetic tests cover counts and budget rejection. Ralph operation `04185d7660a4443997ad3d5b5b83ae93` was accepted and verified. Exact-head [Rust 38837210558](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38837210558), [Platform 38837210510](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38837210510), and [Renderer 38837210605](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38837210605) passed. The prior descriptor repair at `32b0f4cbe36e086ca4c0da8f2ee80b9e967f9cd1` also passed [Rust 38836969425](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38836969425), [Platform 38836969402](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38836969402), and [Renderer 38836969403](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38836969403). Neither improvement qualifies the offline pack or media-license requirements; SRC-003/PACK-003 stay unchecked.

## Inline CSS URL diagnostics — qualified 2026-10-16 UTC

Source `fdb832b5901e8f2f8513ffd76e2daf091c6e6881` adds bounded extraction of simple `url(...)` resource candidates from rendered inline `<style>` elements and `style` attributes, with comment/quoted-string skipping, a CSS text budget, and existing reference-count limits. An old expected candidate count in a synthetic fixture was corrected at `5f77b7fe7cd0addf674a33347ab137bf103a690b`. [Rust 38839825740](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38839825740), [Platform 38839825725](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38839825725) and [Renderer 38839825729](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38839825729) all passed on the corrected exact head. The earlier Renderer failure is a stale fixture count, not a Bridge error.

## HTML preload candidates — qualified 2026-10-16 UTC

Source `1f4c5037734c76aecfcfaa59ed7b01e06019293e` discovers HTML `link[rel=preload]` entries with `as=font|style|image` and responsive `imagesrcset` variants, using the same bounded URL validation and responsive-descriptor guards. Ralph operation `bae227a39ce64065849f401a8f66f7d8` was accepted and exact-path verified. [Rust 38840023413](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840023413), [Platform 38840023217](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840023217) and [Renderer 38840023237](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840023237) all passed. These findings are candidate URL diagnostics, not authenticated downloads, recursive CSS import/font dependency closure, Commons attribution or offline-ready-pack acceptance. `css_and_font_dependencies_verified=false` and `required_media_verified=false` remain deliberate; SRC-003 and PACK-003 are unchecked.

## Further CSS import and image-embed diagnostics — qualified 2026-10-16 UTC

- **Quoted CSS `@import` resources:** direct-master source commit `927f78dd03820555709a9c41773e146a8378b9dc`, Ralph operation `0246e491e33043e0af7d3543b9f13388`, captures basic quoted `@import "sheet.css"` references as distinct `css_import` candidates while skipping comments/ordinary string literals. [Rust 38840358173](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840358173), [Platform 38840358073](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840358073), and [Renderer 38840358066](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840358066) passed at that SHA.
- **Explicitly image-typed embedded resources:** source commit `668295549e0cddbe16c31fc06780708ed734ff44`, Ralph operation `6fa8f2685759443987a8d16c6ed19df5`, records `object[data]`/`embed[src]` references only for declared image MIME types. [Rust 38840552813](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840552813), [Platform 38840552802](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840552802), and [Renderer 38840552703](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840552703) passed at that SHA.
- **Ambiguous HTML resource attribute rejection:** source commit `e83be20d73863cd9fd3fe8afeb751b1a87ed2bef`, Ralph operation `227ca648755349a5b35a6dbd7e20c151`, fails closed for duplicate resource attributes and duplicated inline style attributes rather than silently adopting the last dictionary value. Regression cases cover image, stylesheet, srcset, embedded media and style duplication. [Rust 38840700579](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840700579), [Platform 38840700434](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840700434), and [Renderer 38840700398](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38840700398) passed at the exact head.

These are conservative candidate inventory and fail-closed diagnostics only; complete CSS tokenization, external recursive stylesheet import resolution, authenticated media downloads, per-object source/license attribution, deterministic installed asset closure, and offline device qualification remain unimplemented. `css_and_font_dependencies_verified` and `required_media_verified` remain false; SRC-003/PACK-003 stay unchecked.

## SVG filter image dependency diagnostics — qualified 2026-10-16 UTC

Direct-master source `50521259e6f137efedda463dc9696cc9f0d75cbd` adds `svg_filter_image` candidates for external SVG `feImage` resources and rejects conflicting `href` / `xlink:href` aliases for SVG `image`, `feImage` and `use` instead of silently preferring one reference. Ralph operation `fa26819b126d47f48c8e8661a908725f` accepted and verified both source/test paths. Exact-head [Rust 38846483450](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38846483450), [Platform 38846483456](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38846483456), and [Renderer 38846483461](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38846483461) passed. This remains a diagnostic candidate list, not a verified offline media resolver. SRC-003/PACK-003 remain unchecked.
