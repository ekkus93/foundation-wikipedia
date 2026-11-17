# SRC-003 — public-dump articles rendered, packed and read (2026-11-16)

## Owner decisions (2026-11-16)

1. **Renderer:** revisions named by a public dump are rendered by the free
   public Action API (`action=parse&oldid=<dump revision>`): no key, no paid
   Enterprise access. A self-hosted MediaWiki/Parsoid remains the possible
   later path for fully offline pack building
   ([renderer evaluation](SRC_003_RENDERER_EVALUATION_2026-10-14.md)).
2. **Images:** until PACK-003 downloads image bytes, each required image is
   recorded with its real Commons licence, creator and attribution
   (`MediaAsset` with `sha256: None`) and a `Media` block with its caption;
   the pack does not include image bytes and says so.

## What was built

| Step | Code | Notes |
|---|---|---|
| Acquire and verify a dump generation | existing `wiki-pack fetch-public-generation` | `SHA256SUMS` pinned by digest; every member SHA-256 checked |
| Select articles at their dump revision | `wiki-pack-builder::public_dump_pack::select_dump_pages` | streams the member; pages are used only after the whole member matches its SHA-256; redirect titles resolve one hop and become aliases; missing titles fail |
| Render the exact revision | `render_dump_article` + `online_article::parse_rendered_revision_with_source` | reply must match the dump's page ID, revision ID and title, else `RevisionMismatch` |
| Read into blocks | `wiki-source::rendered_article` | sections h2–h6; paragraphs, lists, tables, quotes, infoboxes; MathML → linear text + allowlisted MathML; furniture skipped |
| Image metadata | `wiki-source::rendered_media` + new canonical `imageinfo` request | licence required; non-upload URLs refused; missing creator stated, not invented |
| Write the pack | `write_dump_pack`, CLI `wiki-pack build-public-dump-pack` | custom `.wpack`, source `public-dump <project>-<generation>`, source pin = `SHA256SUMS` digest |

## Representative run

* Dump: `simplewiki` `2026-11-01`, `SHA256SUMS` sha256
  `1023faa9b883aea03274a9644edaaedb66567b951e8fbf7a07d8b05d228dfd99`
  (computed from our own fetch of that file, not an independent channel),
  member `simplewiki-2026-11-01-p1p1289025.xml.bz2` (487,570,250 bytes,
  sha256 `4176326bf5951d458844694ff7fa8a0da496f97e041f22790f9b48ed6c015ad2`),
  verified by `fetch-public-generation --json`.
* Titles: Pythagorean theorem, Prime number, Gravity, Moon, Tide, Water,
  Photosynthesis, Computer, Albert Einstein, Hypotenuse (maths, physics,
  astronomy, chemistry, biology, computing, a biography).
* Result: 10 articles, each at the dump's revision (e.g. Pythagorean theorem
  page 6327 revision 11017205; Hypotenuse page 44707 revision 8269413);
  77 required images recorded with licences, none included; 87 s on a
  desktop. `wiki-pack verify` / `inspect`: envelope intact, custom origin,
  source `public-dump snapshot simplewiki-2026-11-01`, 10 articles, 0 media.
* Desktop app (Xvfb): the pack reviews as "Custom — integrity verified",
  installs, activates, is found by offline search (Simple English) and reads
  offline: Pythagorean theorem at installed revision 11017205 with its
  Simple English revision source link, sections, lists, citations, figure
  captions, and the formula a² + b² = c² rendered as MathML.

## Defects found and fixed during qualification

* **Desktop installed a different file than the one reviewed** (`40e0c26a`):
  install re-read the path after review. `PackReview` now carries the
  manifest identity digest and desktop install approves only that identity;
  verified in the app ("The file changed after it was reviewed. Nothing was
  installed") and by `a_two_pass_install_refuses_a_pack_other_than_the_reviewed_one`.
* **Formulas vanished from pack articles** (`7b4c3818`): the pack sanitizer
  now keeps allowlisted presentation MathML (`pack_articles_keep_mathml_formulas`).
* **Simple English packs were unsearchable on desktop** (`7b4c3818`): the
  reader filters hits by edition and had no `simplewiki`; the edition list is
  now one module.

## Fidelity, hosting and availability tradeoffs

* **Fidelity:** the HTML is MediaWiki's own rendering of the exact revision,
  so templates, Lua modules, references and math match what Wikipedia
  shows for that revision *today*: an old revision renders with current
  templates (the API does not pin template versions). The revision text and
  identity are the dump's.
* **Availability:** building needs network access to the public API and
  is rate-limited by Wikimedia's API etiquette (one serial request at a
  time here). Reading never needs the network.
* **Independence:** dump checksums are Wikimedia's integrity inventory, not a
  publisher signature; the `SHA256SUMS` pin above came from the same
  origin. Page-census completeness is not claimed.
* **Not covered:** image bytes (PACK-003); Android reading of this pack
  (it shares the same Rust reader and sanitizer); categories and
  cross-article link resolution from the dump.
