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

- **Largest file:** a 7.19 MB animated GIF. It is no wider than 1280 px, so the original was kept; see "Still open".
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

## Still open (PACK-003 not complete)

- **SVG stays scalable:** SVGs are stored as 1280 px renderings, which keeps labels legible when zoomed, but they are not vector. Keeping safe SVGs needs an SVG sanitiser.
- **Raster optimisation:** originals up to 1280 px are kept as they are, including a 7.19 MB animated GIF. Re-encoding large originals is not done.
- **Maps:** interactive Kartographer maps (`maps.wikimedia.org` static images) are not Wikimedia files and are not packed. Static location-map files are packed.
- **Android with a real pack in airplane mode:** shown on the emulator with the synthetic fixture only. Importing a real pack on Android waits for MOBPACK-002.
- **Cross-pack deduplication:** images are deduplicated within a pack. The shared store across packs is STORE-002 (owner hold).
