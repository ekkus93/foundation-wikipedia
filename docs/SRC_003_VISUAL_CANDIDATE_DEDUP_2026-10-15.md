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

## MathML glyph and dynamic-surface diagnostics — 2026-10-16 qualified

Source `e7199187db7783bd0d1d171df41c09410d965c61` detects external `<math><mglyph src>` visual dependencies with a bounded resource count. It additionally records unresolved `canvas`, `iframe` and SVG `foreignObject` surfaces rather than mistaking them for fully inventoried offline visuals. Synthetic regressions cover scope boundaries, inline-only fragment references, duplicate attributes and two independent budgets. Ralph Bridge accepted operation `d0c927a1bec04b338d66584aa8a4e467` from exact head `76193c64555983e2c9ae800a78e2eaac435fa42c`; [Rust 38846881723](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38846881723), [Platform 38846881757](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38846881757) and [Renderer 38846881716](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38846881716) passed on the exact source SHA. Candidate enumeration and dynamic-surface accounting do not constitute CSS/font closure, authenticated downloaded bytes, license attribution or Android offline validation; `required_media_verified=false` and `css_and_font_dependencies_verified=false` remain. SRC-003 and PACK-003 stay unchecked.

## Supplied external stylesheet dependency graph — qualified 2026-10-16 UTC

The diagnostic-only `inventory_css_dependency_graph` accepts caller-supplied stylesheet text keyed by absolute HTTP(S) URL. It resolves supported CSS `@import` links and resource `url(...)` references against each stylesheet URL, traverses cycles without infinite recursion, applies cumulative stylesheet text and reference budgets, and fails closed if a discovered import lacks a supplied source body. The sorted URL-kind candidates have a domain-separated, NUL-delimited SHA-256 identity. The implementation deliberately performs **no network download or source authentication**, is not a full CSS tokenizer, and never asserts verified required media or font closure.

The initial graph implementation `89e74cf1ecf437410d5606bdcaf37cac254d88b8` passed exact-head [Rust 38847577346](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38847577346), [Platform 38847577384](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38847577384), and [Renderer 38847577367](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38847577367). Two independent regression fixture/source-escaping failures were diagnosed from renderer CI and repaired in subsequent direct-master commits `749033d3e33bae1e60046544d0abe052e3caf9e1` (NUL digest test fix) and `9ef44704efe8b53e0c4dbccda35988c76988a895` (single-backslash URL rejection). Final exact-head [Rust 38847968807](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38847968807), [Platform 38847968857](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38847968857), and [Renderer 38847968811](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38847968811) all passed.

The URL guard now rejects unescaped backslashes and DEL control characters. The synthetic regressions cover external stylesheet imports, cycles, relative image/font URLs, absent imported bodies, resource budget, unsupported roots and unsafe URL characters. This does not qualify SRC-003 or PACK-003, nor replace authenticated media downloading, Commons attribution, end-to-end resolver closure or Android airplane-mode checks.

## Rust visual candidate bounds and incomplete-CSS rejection — qualified 2026-10-16 UTC

The portable `wiki-pack-builder::visual_discovery` diagnostic now enforces a hard maximum of 10,000 requested references regardless of caller input. Unterminated CSS block comments and quoted strings in rendered inline styles cause `InvalidInlineCss`, rather than silently yielding a possibly incomplete dependency candidate list. Regression tests exercise malicious/incomplete CSS surfaces and preserve legitimate comment skipping.

The direct-master source change at `9170df438739e8288590d206beec4cba479e0135` (Ralph operation `5bae1089698c4e26ba350abe7ccc2888`) passed [Rust CI 38875821280](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38875821280) and [Platform CI 38875821260](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38875821260), including Rust unit tests. Candidate enumeration remains diagnostic only; real CSS/font closure, authenticated media downloads, provenance/attribution and Android/desktop offline acceptance remain open. The feature does **not** justify checking SRC-003 or PACK-003.

## Malformed CSS URL functions rejected — qualified 2026-10-16 UTC

The Rust candidate scanner now fails closed on `url(` constructs that start a CSS resource function but contain an incomplete or malformed argument, rather than silently omitting potentially required media. The new unit test also proves ordinary `url`-like prefixes embedded in CSS identifiers are not interpreted as resource functions. The initial commit `f14ff8e57457660a893c892ba54742a89487322a` failed only Rust formatting; the exact CI-suggested line wrap was repaired at **`26f286509957c362717ccb66270c51af63d09635`**. The final head passed [Rust 38876441458](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38876441458) and [Platform 38876441484](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38876441484). Ralph write operation IDs: `7dfb107d2a8b4667aaae9db489a70282` (source) and `b3102c738d544d0bb51ee8ba82c5c093` (format fix).

This guards one diagnostic failure mode; it does not establish complete CSS parsing, external stylesheet and font dependency resolution, Commons metadata authentication, verified media-byte acquisition or physical offline acceptance. SRC-003/PACK-003 remain unchecked.

## CLI entry point for the portable Rust candidate scanner — qualified 2026-10-16 UTC

The provisional pack tool exposes a read-only diagnostic command:
`wiki-pack inventory-visuals <rendered.html> <document-url>`. The command reads only a bounded (64 MiB maximum) local regular UTF-8 file, rejects symlink inputs, invokes the Rust `wiki_pack_builder::visual_discovery` scanner with the 10,000-reference cap, and prints JSON including resource kinds/URLs, candidate/reference counts, and explicit false verification flags. It does **not** follow external stylesheets, authenticate rendered source, download media, install content, verify licenses or claim that an offline article/pack is complete.

Initial source SHA `99499b3cece8b50c6c5cd4f5c5eade2f2643d024` encountered a rustfmt-only CI failure ([Rust run 38877063252](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38877063252)), corrected at `1afb373e54a5de882932ead283cc44efd5753227`. Exact-head [Rust 38877172827](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38877172827) and [Platform 38877172782](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38877172782) passed, including CLI tests. Ralph operations: `ed77d42ff00244f5932a8da46a3f72d5` and `04b051e293de4647b2d1f2d9fa6830c8`. SRC-003/PACK-003 remain unchecked.

## Real CLI process-level regression — qualified 2026-10-16 UTC

Added `tools/wiki-pack/tests/visual_inventory_cli.rs` to run the actual `wiki-pack inventory-visuals` binary against a local HTML file. It verifies stable JSON candidate deduplication and explicit unverified-media flags, rejects missing CLI parameters, and rejects invalid UTF-8. Source `24852e09dcbdbfd631ab15884a8caa923fc5bc69` passed [Rust CI 38877608365](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38877608365) and [Platform CI 38877608145](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38877608145). Ralph operation `9da78bd186914619a627c1dbd12a1f30` accepted the direct-master test commit. This remains a diagnostic CLI, not media acquisition, official-source verification or offline-pack/device acceptance. SRC-003/PACK-003 remain unchecked.

## Rust CLI diagnostic fingerprint — qualified 2026-10-16 UTC

The `wiki-pack inventory-visuals` CLI now emits `inventory_sha256` matching the Python diagnostic's versioned, document-URL-scoped candidate-list transcript: `SHA256("foundation-wikipedia/visual-url-candidates/v2\\0" + document_url + "\\0" + canonical_sorted_ASCII_JSON)`. Golden tests cover both direct Rust module invocation and the actual CLI process. An initial rustfmt-only failure at `b56cd535710d5e266b36015261078b7302c93a42` was corrected at `558e68df4a2e7b716afb361a7bb08ba19ea37da5`. Exact-head [Rust CI 38895477271](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38895477271) and [Platform CI 38895477052](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38895477052) passed. Ralph operations `c3e2f74676264e6f91852d453c67611a` and `f8ea8e3428d64e848fb980777d1f42c6` were accepted and path-verified. This digest is neither verified-media object identity nor official source authentication; SRC-003/PACK-003 remain unchecked.

## Portable Rust supplied CSS graph inventory — qualified 2026-10-16 UTC

`wiki_pack_builder::visual_discovery::inventory_supplied_css_graph` now traverses canonical absolute HTTP(S) stylesheet roots and caller-supplied CSS text. The diagnostic follows `@import` candidate edges, processes cycles once, resolves relative CSS `url(...)` candidates against each stylesheet, rejects missing imported content and unsafe/noncanonical URL identities, and enforces aggregate CSS text, stylesheet and reference budgets. Tests include cyclic graphs, missing imports and budget failures. No CSS is fetched; `css_and_font_dependencies_verified=false` and `required_media_verified=false` remain mandatory.

Initial commit `a73e418534de24d3ea9993d0523c3bc78edafcd8` had a formatting-only CI failure. Corrected code `816965fac5cfecde4aab8f959c0cfbce01c7043a` passed [Rust 38896545373](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38896545373) and [Platform 38896545399](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38896545399). Ralph operations `3de120ab0cdd4151b43843a3616a36db` and `418db15b5f084262a06592b8c395791e` were accepted and file-verified. SRC-003/PACK-003 remain unchecked.
