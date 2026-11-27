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

## Update — the pack's own licence (2026-11-27)

Code: `ddb2758d`, `0e1a40b9`. This closes the first two gaps below; the Android top bar now says Foundation Library (`bd671f34`).

- **Reader:** the attribution section states the text licence the pack's manifest declares, not a constant. The installer already records `license.text_spdx` in each snapshot's `manifest.json`. `wiki_store::pack_install::installed_text_license` reads it back when the snapshot is opened, accepting only an SPDX-shaped identifier. It is cached with the open snapshot and passed to `attribution_section_for_license`, so the section follows the active pack. Known identifiers get their name and deed link: CC BY-SA 4.0 and 3.0, CC BY 4.0, CC0 1.0 and GFDL 1.3 or later. Any other identifier is shown escaped and unlinked ("the licence the pack names as MIT"). A missing or invalid record says "a licence this pack does not state" rather than guessing one.
- **Install review:** both the Android dialog (custom and official packs) and the desktop import review now show one plain-text line before anything is installed. It is built in Rust by `pack_license_notice`, for example "Article text: CC BY-SA 4.0. 12 images, each credited with its creator and licence. Credits are shown at the end of every article."
- **Tests:**
  - unit tests for known, lower-case, unknown, missing and hostile identifiers and for the review notice;
  - a `wiki-store` test that reads back the recorded licence and returns `None` once the record is tampered with, nulled or removed;
  - the FFI test `the_attribution_states_the_licence_the_active_pack_declares`, which installs a CC-BY-SA-3.0 pack and an MIT pack and switches between them;
  - `PackTrustUiTest` checks the notice is displayed (passed on the API 35 emulator).

## Update CI

On `ddb2758d` (reader licence): all five workflows passed, incl. [Rust workspace 46017894284](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46017894284) and [Android FFI bridge 46017894023](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46017894023). On `0e1a40b9` (install review): all six passed, incl. [Rust workspace 46018735656](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46018735656), [Desktop launch smoke 46018735660](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46018735660) and [Android FFI bridge 46018735760](https://github.com/ekkus93/foundation-wikipedia/actions/runs/46018735760) (`OK (48 tests)`).

## Not done

- The per-image notices are shown in the reader, not listed one by one in the install review (the review gives the count).
- Internal identifiers (`org.foundation.wikipedia`, the desktop data folder, the repository name) still use the old name; renaming them is due before the first public release.

## CI

CI on `65a3d685` (all of this work plus the rename): all nine workflows passed, incl. [Rust workspace 45961745860](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745860), [Android FFI bridge 45961745759](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745759), [Desktop launch smoke 45961745887](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745887), [Platform shells 45961745775](https://github.com/ekkus93/foundation-wikipedia/actions/runs/45961745775) (`OK (48 tests)`).
