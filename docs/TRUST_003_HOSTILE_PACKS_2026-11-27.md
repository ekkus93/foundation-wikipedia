# TRUST-003 — hostile-pack suite and display limits (2026-11-27)

Code: `98422453` (fixes), `3ef01d8a` (suite), `e0708ea1` (display limits). TRUST-003 remains unchecked (it depends on PACK-004).

## Suite

`crates/wiki-ffi/tests/hostile_packs.rs` (47 tests) and `tools/wiki-pack/tests/hostile_packs_cli.rs` (4 tests) build hostile packs in code (no binary fixtures). Each attack goes through `install_pack` and `install_pack_stream`; it must end in the expected typed error, leave the library directory tree unchanged (no file outside it, no staging residue) and leave an already-installed good pack usable. The store-level `install_custom_pack` and `install_custom_pack_stream` must agree on the error. The CLI (`verify`, `inspect`, `--json`, a byte budget, `stage-unsigned`) must exit non-zero without a panic and leave no residue. Signed packs use a throwaway test key; the publisher key was not used.

| Attack | Refused by |
|---|---|
| Path traversal, absolute paths, backslashes, NUL, drive letters, UNC, NTFS streams, Windows device names, bidi/zero-width/BOM/NFD/noncharacter names, over-long or deep paths | `ManifestError::UnsafeObject` |
| Case or prefix aliases; hostile pack ID, snapshot, definition or project | `DuplicateObject`; `UnsafeMetadata` |
| Reserved installer names, symlinked library directories, a symlink at the target snapshot, reinstalling over an installed pack | `UnsupportedObject`, `Storage`, `AlreadyInstalled` |
| Length, count and size bombs in the envelope and manifest; spool quota; endless or failing streams | `ResourceBudget`, `VerificationFailed`, `InvalidInput` |
| Lying sizes, truncation at every boundary, trailing bytes, bad headers, flipped or swapped object bytes, every manifest prefix and flipped byte | `ObjectDigest`, `Io`, `TrailingBytes`, `InvalidHeader`; never a panic |
| Schema-version and encoding attacks; metadata lies; official claim without a signature; garbage catalog or shards with valid hashes | `ManifestError`, `InvalidPackMetadata`, `UnsignedOfficialPack`, `VerificationFailed` |
| 37 hostile SVGs (scripts, handlers, `foreignObject`, external references, CSS, animation, entity bombs, node bombs, deep nesting) | `PackMediaError::UnsafeSvg` |
| Media index JSON attacks (26 cases); media that does not match the articles | `Media(...)`, `ObjectMismatch`, `MissingImage`, `UndeclaredMedia` |
| Hostile article HTML with valid hashes | Installs; sanitised when displayed (no script, handler, frame, form or unsafe link reaches the page) |
| Signed packs with any of the above | Same errors as unsigned: a signature does not excuse content |
| Bad signatures (flipped, other pack, unknown, revoked or expired key, forged key ID) | `PublisherVerificationFailed`, `NotOfficialPack` |

## Holes found and fixed (`98422453`)

1. **Deeply nested SVG crashed the process.** A pack SVG about 800 elements deep overflowed the stack (SIGABRT); the depth limit was checked only after parsing. `pack_svg::element_nesting_exceeds` now counts nesting before parsing.
2. **Fake images were accepted.** HTML, script, SVG or program bytes declared as PNG/JPEG/GIF/WebP installed, as did one raster declared as another. `check_pack_media` now compares the bytes' signature with the declared type (`PackMediaError::SpoofedImage`) and refuses PNG/GIF headers claiming more than 64 megapixels.

The three real packs built earlier (simplewiki science, maps, audio/video) still install and pass their audit.

## Display limits (`e0708ea1`)

The suite also found that displaying article HTML takes time that grows with the square of its nesting depth and faster than linearly with character references. Release-build timings: a real 70 KB article (about 15 deep, about 300 references) 3 ms; 16,000 nested elements 0.5 s; 160,000 references 0.25 s. A hostile pack could freeze the reader for minutes.

Every reader sanitiser now runs a single linear check first. HTML more than 512 elements deep (the extractor's limit), with more than 262,144 character references, or over 8 MiB is shown as "This article is too large or too deeply nested to display safely." without being parsed. Comments, quoted attributes and void or self-closed elements do not count as nesting. Tests: `html_beyond_the_display_limits_gets_a_notice_quickly` (all three sanitisers); the suite now includes a 200,000-deep pack and 1.5 million references, both shown as the notice (whole test 7 s in a debug build).

## Still uncovered

- Raster polyglots with a valid header (a GIF89a header then HTML, declared GIF) install; they are served only as images under `PACK_IMAGE_CSP` and `nosniff`. JPEG and WebP have no dimension limit.
- `OfflineLibrary::open_page` returns stored article HTML unsanitised; only the reader-document paths sanitise. The Android app does not render it, but no future caller may.
- Packs over the display limits still install; the notice appears only when the article is opened.
- SQLite content engineered to be slow, Windows/macOS filesystems, concurrent writers, and the download path.

## CI

CI on `65a3d685` (all of this work plus the rename): all nine workflows passed, incl. [Rust workspace 45961745860](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745860), [Android FFI bridge 45961745759](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745759), [Desktop launch smoke 45961745887](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745887), [Platform shells 45961745775](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745775) (`OK (48 tests)`). On `e0708ea1` (display limits): [Rust workspace 45962268438](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45962268438), [Platform shells 45962268522](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45962268522), [Desktop launch smoke 45962268388](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45962268388), [Android Rust ABI qualification 45962268429](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45962268429) passed; [Android FFI bridge 45962268464](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45962268464) failed only on the two `PackImagesUiTest` pixel checks (the recurring CI-only "figure not drawn": page loaded, image served, no red on screen; no system dialog this time), unrelated to this change. `1becd362` saves a screenshot when it recurs.
