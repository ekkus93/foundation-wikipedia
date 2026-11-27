# SEC-002 — attribution in the offline readers (2026-11-27)

Code: `a64f0eea`. SEC-002 remains unchecked.

## Before

Android showed only "Revision N · offline copy"; the desktop offline reader showed the article body alone.

## Now

`wiki_source::attribution::attribution_section` builds one section from the stored article, so it works with no Internet, and both readers append it (Android through `wrap_document`; desktop through the new `ArticlePage.attribution_html`):

- the text licence (CC BY-SA 4.0, linked), links to the exact revision and to its history and contributors on the article's Wikipedia;
- "Media credits": one entry per pack image or drawn map, with file name, creator, licence, a link to its file page, and a plain note of what the pack did to it (converted to WebP without loss; first frame of an animation; vector with scripts and editor data removed; map drawn from open data);
- "This app is not made by or affiliated with the Wikimedia Foundation. Wikipedia is a trademark of the Wikimedia Foundation."

All text is escaped; a link is shown only if it is `https` (or built here from a valid language code).

Seen on the API 35 emulator: *Hypotenuse* from the real simplewiki pack ends with the section and the credit for `Rtriangle.svg` (Gustavb, CC BY-SA 3.0, "Vector image with scripts and editor data removed").

**Naming:** the agent raised that "Foundation Wikipedia" could suggest affiliation; the owner chose to rename the app to **Foundation Library** (visible name only, `932c1015`). Internal identifiers keep the old name until the first public release.

## Tests

4 unit tests in `attribution.rs` (licence and links; every image credited with its derivation; maps credited with their own creator and licence; hostile metadata escaped, bad links dropped); the Android document carries the section in both themes; the desktop page ends with it; the pack-built article test checks the credit, licence and file page of every embedded image.

## Not done

- The text licence is a constant: `Article` has no licence field, so the pack manifest's licence is not passed through.
- Pack-level notices (the `media/index.json` attribution object) are not shown in the install review.
- The reader's top bar on Android still says "Wikipedia".

## CI

CI on `65a3d685` (all of this work plus the rename): all nine workflows passed, incl. [Rust workspace 45961745860](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745860), [Android FFI bridge 45961745759](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745759), [Desktop launch smoke 45961745887](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745887), [Platform shells 45961745775](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745775) (`OK (48 tests)`).
