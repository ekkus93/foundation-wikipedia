# PACK-003 — article images in packs, shown offline (2026-11-17)

PACK-001 was ruled done by the owner on 2026-11-17, which unblocked this work. Images come from the free public APIs, following the owner's 2026-11-16 SRC-003 renderer decision. Paid Enterprise data (SRC-002) is not used.

## What was built

| Step | Code | Behaviour |
|---|---|---|
| Choose what to download | `wiki_source::media_download` | Uses the `imageinfo` request (`url\|mime\|size\|sha1\|extmetadata`, `iiurlwidth=1280`). How each file is fetched is listed below this table. |
| Fetch | `MediaFileTransport`, `CurlTransport::get_media` | HTTPS only, to `upload.wikimedia.org` and `thumb.wikimedia.org`. Canonical `/wikipedia/<wiki>/…` paths only: no query, port or credentials. No redirects. 8 MiB cap. `Accept` limited to image types (no header injection). |
| Check | `verify_image` | An original must match its stated format, size and SHA-1. Any download must start with a PNG, JPEG, GIF or WebP signature; its stored type comes from that signature. |
| Find required images | `rendered_article` (fixed in `a1030f79`) | Every displayed Wikimedia file outside page furniture, including figures without a file link (`link=`) and inline images such as flags. Formula fallback images are excluded because the MathML is kept. |
| Build | `public_dump_pack::{DumpMediaStore, render_dump_article, write_dump_pack}`, CLI `build-public-dump-pack` | Each image is fetched once per URL and stored once by SHA-256. The build fails if any image cannot be fetched or verified, or if an article image has no bytes. |
| Pack layout | `wiki_store::pack_media` | Objects `media/<sha256>`, plus the canonical index `media/index.json` (`{version, images:[{sha256, mime, bytes}]}`, sorted, exact-form only), declared as the manifest attribution object. `counts.media` is the number of distinct images. Licence, creator, attribution and source URL stay on each article's `MediaAsset`. |
| Install | `pack_install` + `check_pack_media` | The pack must hold exactly the images its articles require, each in the stated format, with no extras. A missing, mislabelled or unused image is refused (`PackInstallError::Media`). A text-only pack may neither carry nor claim images. |
| Reopen | `installed_shard_inventory` | Images are never treated as article shards, the same rule the installer applies (`is_media_object`). |
| Read | `read_pack_image`, FFI `OfflineLibrary::read_image` | Looks images up by digest only. Re-hashes them on every read. Bytes changed after install are refused. |
| Show | `reader_document::sanitize_pack_article_html` | An `<img>` keeps a source only if its Wikimedia file is in the pack. That source becomes `<base><sha256>`, and `srcset` is dropped. No other attribute may use a local image URL. |
| Android | `ArticleReader` | Answers `https://foundation.invalid/media/<sha256>` for sub-resource requests only, from the active pack. |
| Desktop | `packimage` URI scheme, `offline_packs::pack_image_response` | Serves `GET packimage://localhost/<sha256>` only. The pack frame's CSP is `img-src packimage: http://packimage.localhost`; Windows uses the second form. |

How each file is fetched:

- **PNG, JPEG, GIF or WebP no wider than 1280 px:** the original file, checked against Wikimedia's size and SHA-1.
- **Anything else** (SVG, wider rasters, TIFF and similar): Wikimedia's 1280 px rendering. Its format is negotiated, with WebP preferred; the thumbnail service returns WebP behind a `.png` URL when WebP is accepted.

## Real run

- **Source:** simplewiki dump 2026-11-01.
  - `SHA256SUMS` pin `1023faa9…`.
  - Member `simplewiki-2026-11-01-p1p1289025.xml.bz2`, pin `4176326b…`.
- **Articles (10):** Pythagorean theorem, Prime number, Gravity, Moon, Tide, Water, Photosynthesis, Computer, Albert Einstein, Hypotenuse. These are the same titles as the SRC-003 evidence.
- **Result:** "All 91 article images are included (91 distinct files)". The pack is 28,069,098 bytes, against 356,154 bytes for the text-only pack. The article shard is 332,124 bytes.

| Format | Files | Bytes |
|---|---|---|
| JPEG | 44 | 11.29 MB |
| GIF | 3 | 7.73 MB |
| PNG | 12 | 6.11 MB |
| WebP | 32 | 2.55 MB |

- **Largest file:** a 7.19 MB animated GIF. It is no wider than 1280 px, so the original was kept; see "Raster re-encoding".
- **Audit** (`cargo test -p wiki-ffi --test pack_images audit -- --ignored`, with `PACK003_PACK` set): for every article, every required image read back verified and was shown locally. No other image source remained (26.7 MB read back).
- **Images left unloaded:** listed with `PACK003_SHOW_UNUSED`, these were navbox images, the Wikidata edit pencil, sister-project and portal icons, and formula fallbacks (shown as MathML).
- **The audit found two bugs before this run, both now fixed with regression tests:**
  - Figures without a file link and inline images had not been required (`every_displayed_wikimedia_file_is_required_even_without_a_file_link`).
  - A single-file `imageinfo` reply offering older file versions was refused.

## In the app

**Desktop, network removed.** The app and Vite ran inside `unshare -rn`, a network namespace with only loopback: `curl` to Wikipedia exited 6 there. A connect/DNS logger was preloaded.

- The pack was imported through Settings › Wikipedia & Offline Data (review: Custom — integrity verified, 10 articles), then installed and activated, and Work offline was turned on.
- These articles showed their images from the pack:
  - Pythagorean theorem: the squares diagram with its *a*, *b* and *c* labels.
  - Moon: the photo, plus the infobox symbol.
  - Hypotenuse: the labelled triangle that was previously missed.
- The logger recorded only connections to Vite on `[::1]:1420`.
- **Bug found and fixed:** activation first failed with "failed verification", because reopening a snapshot counted images as article shards. The regression is now checked in `a_dump_named_revision_becomes_an_installable_pack` and `pack_images`.
- App data was backed up beforehand and restored afterwards.

**Android, API 35 emulator.** `PackImagesUiTest` installs the committed fixture `android-images-v2.wpack`, a one-article pack whose figure is a 32×32 red PNG. Then:

- it opens the article and finds the red figure drawn on screen;
- the image request returns the pack bytes, whose SHA-256 is checked;
- an unknown digest, a main-frame navigation to the image URL, and a Wikimedia URL are all refused with 403.

The full suite gives `OK (41 tests)`, and CI now expects 41.

## CI (exact head)

- **`9fd0525a`** (images in packs): [Rust workspace 44072869103](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869103), [Platform shells 44072869163](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869163), [Desktop launch smoke 44072869043](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869043), [Android FFI bridge 44072869212](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869212), [Android install smoke 44072869057](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869057), [Android Rust ABI qualification 44072869218](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869218), [Android record codec probe 44072869040](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44072869040).
- **`7cc27dee`** (Android selection test fix, after `0c4171b5` failed it): [Rust workspace 44073049532](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44073049532), [Platform shells 44073049451](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44073049451), [Android FFI bridge 44073049602](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44073049602), [Android install smoke 44073049688](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44073049688).
- **`86d691eb`** (lossless re-encoding and labelled stills): [Rust workspace 44192888549](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888549), [Platform shells 44192888392](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888392), [Desktop launch smoke 44192888579](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888579), [Android FFI bridge 44192888403](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888403), [Android install smoke 44192888577](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888577), [Android Rust ABI qualification 44192888623](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888623), [Android record codec probe 44192888525](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44192888525).
- **`f54aa7f4`** (SVG kept as sanitised vectors): [Rust workspace 44210789505](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789505), [Platform shells 44210789445](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789445), [Desktop launch smoke 44210789473](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789473), [Android FFI bridge 44210789520](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789520), [Android install smoke 44210789499](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789499), [Android Rust ABI qualification 44210789438](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789438), [Android record codec probe 44210789459](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789459), [Dependency lock refresh probe 44210789453](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44210789453). The FFI bridge's first attempt failed: `PackImagesUiTest` waited 15 s for the red figure to be drawn on the CI emulator. It passed on rerun and 4 of 4 times locally; that wait is now 30 s, the same as the page load.

## Tests added

- **`media_download` (6):** download choice on the real fixture, original versus rendering, refused hosts and formats, format, size and hash checks, mapping an article image source to its file, canonical URLs only.
- **`online_curl`:** media URLs only, and `Accept` cannot inject a header.
- **`pack_media` (2):** canonical index, completeness rules, verified reads.
- **`reader_document`:** pack images load only from the local URL by digest, and forged local URLs in links or images are removed.
- **`public_dump_pack`:**
  - builds from the real Hypotenuse revision with its images;
  - installs, reopens, and reads every image;
  - every required figure is local and nothing else loads;
  - a failed download fails the article;
  - an article without bytes cannot be packed.
- **`wiki-ffi pack_images`:** install → activate → `open_page.images` → `read_image` → reader document; tampered bytes are refused; the committed Android fixture is pinned.
- **Desktop:** frame CSP for pack images; the citation view maps image URLs to the desktop scheme.
- **Android:** `ReaderLinksTest.onlyExactPackImageRequestsAreRecognised`, `PackImagesUiTest`.

## Raster re-encoding (`86d691eb`)

**Owner decision (2026-11-17):** "Lossless + still for big GIFs". Two files were 38% of the first real pack: a 7.2 MB animated GIF (*Water triple point*, 480×270, 156 frames) and a 3.5 MB PNG map (*Nile basin*). Wikimedia's own smaller renderings barely helped: the GIF is still 3.3 MB at 330 px, and renderings only come in fixed widths below the original.

| Source | Packed as | Check |
|---|---|---|
| PNG, single-frame GIF | Lossless WebP, only when smaller (`LosslessWebp`) | Decoded again; must give exactly the same RGBA pixels |
| PNG with an ICC profile, non-sRGB gamma or chromaticities, APNG, 16-bit | Unchanged | Lossless WebP (sRGB, 8-bit) could not reproduce it exactly. Gamma and chromaticities that only restate sRGB, as the Nile map's do, are allowed |
| Animated GIF over 2 MiB | First frame as lossless WebP (`FirstFrameStill`) | The reader adds "Still image: the animation is online only." after it |
| Smaller animated GIF, JPEG, WebP | Unchanged | Lossless re-encoding would only grow them |
| Anything that will not decode within the limits (16,384 px, 256 MiB) | Unchanged | Re-encoding never drops or alters an image |

- **Code:** `wiki_pack_builder::image_reencode`, run on verified bytes in `DumpMediaStore`. Only the publisher-side builder depends on the pure-Rust `image` crate, pinned to 0.25.6 because later releases need Rust 1.88. The app's reader decodes nothing new; WebP was already a pack format.
- **Attribution:** `MediaAsset.derivation` (`lossless_webp` / `first_frame_still`) records how the stored bytes were made from the file at `source_url`. It is omitted when absent, so older records decode and encode unchanged. It is carried to `StructuredMediaNotice` and to the FFI page (`still_images`). A model check refuses a derivation on anything but stored WebP.
- **Real pack, rebuilt:** 28,069,098 → **19,970,682 bytes (−29%)**, all 91 images still included. The audit read every image back verified and shown locally (19.6 MB of images).

| Stored | Files | Bytes |
|---|---|---|
| JPEG (unchanged) | 44 | 11.29 MB |
| WebP rendering (unchanged) | 32 | 2.55 MB |
| PNG (unchanged: ICC profile) | 7 | 2.50 MB |
| Lossless WebP | 5 | 2.52 MB (Nile map 3.51 → 2.45 MB) |
| GIF (unchanged, small) | 2 | 0.54 MB |
| First-frame still | 1 | 0.18 MB (was 7.19 MB) |

- **Desktop, network removed** (same namespace setup, `curl` exit 6; logger: only `[::1]:1420`): the rebuilt pack imported (review: 19.0 MB, integrity verified), installed and activated. In *Water*, the hydrogen-bond diagram (lossless WebP) displayed. The triple-point figure displayed as its first frame with "Still image: the animation is online only." above its caption "Water as a solid, liquid and gas at the same time". App data was backed up and restored.
- **Android:** bindings regenerated for `still_images`; `OK (41 tests)` on the API 35 emulator.
- **Tests:** `image_reencode` (7): identical pixels, PNG colour chunks kept unless sRGB-equivalent, oversized chunk lengths, still versus small animation, JPEG/WebP untouched. `diagrams_are_packed_as_lossless_webp_and_say_so`: build → install → the installed record keeps the derivation → images read back as WebP. `an_animation_packed_as_a_still_is_labelled_where_it_is_shown` (reader).

## SVG kept as vectors (`f54aa7f4`)

**Owner decision (2026-11-17):** build the SVG sanitiser so diagrams stay zoomable, rather than accept 1280 px rasters.

| Stage | Code | Rule |
|---|---|---|
| Download | `media_download` | An SVG up to 4 MiB is fetched as the original (`Accept: image/svg+xml`) and must match Wikimedia's size and SHA-1. Its 1280 px rendering is the fallback. A larger SVG goes straight to the rendering. |
| Sanitise | `wiki_store::pack_svg::sanitize_svg` | The SVG is rebuilt (a fresh serialisation) from an allowlist of drawing elements: shapes, text, gradients, patterns, clip paths, masks, markers, filters and `<style>`. Metadata, `title`/`desc`, editor namespaces (Inkscape, Sodipodi, RDF), comments, whitespace between elements and Inkscape `flowRoot` (drawn by no browser and not by Wikimedia) are dropped. A link `<a>` becomes a group. |
| Refuse → raster | same | Refused, so the rendering is packed instead: `script`, `foreignObject`, `image`, animation, fonts or any unknown element; `on*` handlers; any `href` not `#id`; any `url(…)` not `url(#id)`; any scheme (`javascript:`, `data:`, `http(s):`, `//`); CSS with `@`, `\`, `expression`, `binding`, `behavior` or `image(`; `xml-stylesheet` or any processing instruction; any DTD declaration other than plain text entities (no nesting, parameter or external entities; expansion capped at 4 MiB); depth over 128, over 200,000 nodes, or over 1 MiB after sanitising. |
| Install | `check_pack_media` | Every packed SVG must be exactly what the sanitiser makes of it (`is_clean_svg`); otherwise `PackMediaError::UnsafeSvg`, shown as `MediaInvalid`. A hostile custom pack cannot bypass the builder. |
| Show | `<img>` only | Browsers run no script and load nothing for an SVG used as an image. Both apps also send `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox` and `nosniff` with every pack image (`PACK_IMAGE_CSP`; a Rust test pins the Android constant to it). |

- **Attribution:** `MediaDerivation::SanitizedSvg`; `source_url` still names the original file.
- **XML parser:** `roxmltree` 0.20 (MIT/Apache-2.0, no dependencies), with DTDs refused; the sanitiser expands plain entities itself first.
- **Real SVGs** (the 32 required by the 10-article pack):
  - 31 are kept as vectors. The one exception is a 1.1 MB poster that would be over 1 MiB after sanitising; the CLI reports it ("kept as its 1280 px rendering … (TooLarge)").
  - The original first run refused 10 of them for having a DOCTYPE; plain-entity DOCTYPEs (Adobe Illustrator, SVG 1.1) are now accepted.
  - **Fidelity:** each original and its sanitised form were rendered as `<img>` at 800 px in headless Chromium, with 0 differing pixels for all 31 (out of 3,418–600,936 drawn pixels each).
- **Real pack, rebuilt:** 19,970,682 → **18,566,930 bytes**. The 31 SVGs total 0.91 MB, against 2.55 MB as WebP renderings. All 91 images are still included, and the audit read every image back verified and shown locally.
- **Desktop, network removed** (loopback-only namespace, `curl` exit 6; logger: only `[::1]:1420`): the pack (review: 17.7 MB) installed, which passed the install-time SVG check for all 31, and activated. *Pythagorean theorem*'s squares diagram (`Pythagorean.svg`, now a vector) displayed with its labels. App data was backed up and restored.
- **Android:** the committed fixture now also holds a blue 32×32 SVG figure. `PackImagesUiTest` finds both the red PNG and the blue SVG drawn on screen, and checks both responses' bytes, type and CSP; `OK (41 tests)`.
- **Tests:**
  - `pack_svg` (5): a real-style Inkscape drawing keeps everything drawn; scripts, handlers, foreign content, external references and unsafe CSS are refused; DTD bombs, external entities and `ATTLIST` defaults are refused; Illustrator DOCTYPE entities are expanded; values survive a round trip exactly.
  - `media_download`: SVG originals use the committed Commons fixtures (`fixtures/wikimedia/commonswiki/`, hash-checked against the recorded imageinfo); a changed byte fails.
  - Builder: Hypotenuse builds with three sanitised SVGs; a hostile SVG with a matching hash falls back to its rendering, and changed bytes fail.
  - FFI: `a_pack_with_an_unsanitised_svg_is_refused` covers script, handler, external image, a harmless comment (not exact form), and HTML labelled as SVG.

## Interactive maps (`e38bf53a`, `e99dceb6`)

**Why they can't come from Wikimedia:** interactive (Kartographer) maps are drawn by Wikimedia Maps (`maps.wikimedia.org`), not stored as Commons files. Its [terms of use](https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use_for_Wikimedia_Maps) say "Wikimedia Maps may not be used by third-party services outside of the Wikimedia projects". They also prohibit "Excessive downloading (such as downloading significant areas of tiles for later offline usage)". OpenStreetMap's public tile service has a similar rule.

**Owner goal (2026-11-17):** the app must work with no Internet at all. Packs therefore draw their own maps from open map data, and an online-only note is only the fallback.

**Stage A — drawn from Natural Earth (`e99dceb6`):**

- **Base map:** `wiki_pack_builder::map_render` uses Natural Earth v5.1.2 (public domain). Its 12 GeoJSON layers are pinned by SHA-256 (`NATURAL_EARTH_FILES`) and loaded with `build-public-dump-pack --natural-earth <dir>`:
  - land, lakes, rivers, country and state borders, and places;
  - the 1:50m layers for zoom 0–4, the 1:10m layers beyond;
  - place names shown by their `min_zoom`.
- **Article map data:** markers, lines and shapes come from `prop=mapdata` through the Action API (`WikipediaRequests::map_data`), never the maps service.
  - The request names exactly the content-hashed groups in the pinned revision's `data-overlays`, and `parse_map_data` requires exactly those groups for that page.
  - If the article's maps changed since then, nothing is drawn and the reason is reported.
  - Simplestyle colours and widths are validated; hostile values fall back to defaults.
- **Drawing:** Web Mercator at the article's centre, zoom and size, the same area Kartographer shows. A map with no stated centre fits its shapes.
  - Geometry is clipped to the frame (Sutherland–Hodgman and Liang–Barsky) and thinned below half a pixel.
  - Markers are drawn as pins, with "Natural Earth" credited in the corner.
  - The SVG then goes through the pack SVG sanitiser like any other.
- **Stored:** each map is a `MediaDerivation::RenderedMap` asset (SVG, CC BY-SA for the article's shapes plus public-domain base). Assets are in document order, after the article's images; the content digest is recomputed.
- **Shown:** the stored, sanitised HTML holds each map as a note, in document order. Any `foundation-map-note` class the article itself writes is renamed, so only real notes match.
  - In a pack, the n-th note becomes the n-th drawn map (`map:n` in `PackImageSources`), with "Map drawn offline from Natural Earth (public domain)" under it.
  - Without a drawing, or online, the note stays: "Map (online only)", its stated centre, and a View on OpenStreetMap link. Nothing is ever loaded from the maps service.
- **Real run:** a 2-article simplewiki pack (*Tokyo* rev 10653989, *Eiffel Tower* rev 11033818) built with both maps drawn: Tokyo 5,050 bytes, Eiffel Tower 583 bytes (fitted to its marker at zoom 16). The CLI reported "Tokyo: 1 map shape(s) from Wikimedia Maps not drawn" (its Wikidata outline).
  - Load time: the base map in 0.9 s; each map in about 15 ms.
  - The audit found every image and both maps shown locally, with no note left and no maps-service reference.
  - Renders of real data (Tokyo z8, Paris z5, world z1) were reviewed in Chromium. Coasts, bays, rivers, borders and city names are correct.
- **Desktop, network removed:** in *Tokyo* the drawn map (Kawagoe, Hachiōji, Tokyo, Kawasaki, Yokohama, Tokyo Bay) showed under the packed "Location within Japan" map. The logger recorded only `[::1]:1420`. App data was backed up and restored.
- **Tests:**
  - `map_render` (4): centre, zoom, marker, labels, clipping, fitting, counted `ExternalData`, hostile styles.
  - `map_data` (2) and `online_requests::map_data_is_read_for_exact_groups_through_the_action_api`.
  - `interactive_maps_are_drawn_from_the_base_map_and_the_articles_data` (builder, including changed map data).
  - `a_pack_shows_its_drawn_maps_in_place_of_the_stored_notes` (reader, including a forged note and placeholder).
  - `interactive_maps_become_a_labelled_note_and_nothing_is_loaded` (the fallback).
- **Not yet (Stage B):**
  - Street-level detail at high zoom (the Eiffel Tower map is its marker on plain land).
  - Shapes Kartographer loads from Wikimedia Maps (`ExternalData` geoshape/geoline by Wikidata ID).
  - Both need OpenStreetMap data (ODbL) from a regional extract.

**CI:**

- **`e38bf53a`** (note fallback): [Rust workspace 44244817113](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44244817113), [Platform shells 44244817281](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44244817281), [Desktop launch smoke 44244817090](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44244817090), [Android Rust ABI qualification 44244817257](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44244817257). The FFI bridge ([44244817166](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44244817166)) failed in `PackImagesUiTest`, the intermittent CI-only figure timeout. Diagnostics were added in `76cc8b1f`, whose [Android FFI bridge 44246239285](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44246239285) passed.
- **`e99dceb6`** (drawn maps): [Rust workspace 44248312582](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44248312582), [Platform shells 44248312816](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44248312816), [Desktop launch smoke 44248312621](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44248312621), [Android FFI bridge 44248312578](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44248312578), [Android Rust ABI qualification 44248312427](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44248312427), [Android record codec probe 44248312600](https://github.com/ekkus93/foundation-wikipedia/actions/runs/44248312600).

## Still open (PACK-003 not complete)

- **SVG:** done (above). An SVG over 1 MiB after sanitising, or one using refused features, is still a 1280 px rendering.
- **Raster optimisation:** done losslessly (above). JPEGs and PNGs with an embedded ICC profile are kept as they are; the largest remaining file is a 2.45 MB lossless map.
- **Maps:** drawn offline from Natural Earth (above). Street-level detail and Wikidata shapes need OpenStreetMap extracts (Stage B). A map you can pan and zoom offline is FUT-007.
- **Android with a real pack in airplane mode:** shown on the emulator with the synthetic fixture only. Importing a real pack on Android waits for MOBPACK-002.
- **Cross-pack deduplication:** images are deduplicated within a pack. The shared store across packs is STORE-002 (owner hold).
