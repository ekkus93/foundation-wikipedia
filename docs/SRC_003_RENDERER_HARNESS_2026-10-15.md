# SRC-003 — MediaWiki/Parsoid renderer fixture harness (2026-10-15)

**Status:** fixture harness qualified on exact-head CI (see below); the
renderer itself is not yet qualified for real dump content. Canonical TODO
`SRC-003` remains unchecked. This implements the "version-pinned MediaWiki/Parsoid
fixture service" proposed in
[SRC_003_RENDERER_EVALUATION_2026-10-14.md](SRC_003_RENDERER_EVALUATION_2026-10-14.md).

## What it does

`tools/renderer-harness/run.sh` starts the official `mediawiki:1.43` Docker
image (bundled Parsoid, Cite, ParserFunctions, Scribunto with the standalone
Lua engine) on SQLite, imports synthetic fixture pages (a template, an infobox
template, a Lua module, an English article using all of them, Spanish and
Chinese articles, and a redirect), then fetches Parsoid HTML for each through
`/rest.php/v1/page/{title}/with_html` twice.
`scripts/check_renderer_fixtures.py` fails closed when expected content is
missing, when unexpanded markup or a Lua/template error appears, when page or
revision identity is absent, or when the two passes differ.

The `renderer-qualification` workflow runs it and retains `evidence.json`
(page ID, revision ID, HTML SHA-256 and size per page), `siteinfo.json`
(installed MediaWiki and extension versions), the image digest and the
container log for 14 days.

## Evidence

- First run, `c4eead34cf9c14631da685881b51d02bda3c2554`
  ([run 38799303082](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38799303082)),
  **failed** on two checker assumptions, not renderer defects: Parsoid wraps
  each parser-function result in its own span (raw-HTML substring match split
  `yes / 2`), and the redirect `Location` header is percent-encoded. Template,
  Lua, reference, table and Unicode fixtures already rendered correctly.
- Fix and qualification, `1e1b003a64d1118689e0ef856d8210aa0a249c2a`:
  [renderer harness run 38799628018](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38799628018),
  [Rust workspace 38799628022](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38799628022) and
  [Platform shells 38799628013](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38799628013) all passed.
- Environment (artifact `renderer-fixture-evidence`): MediaWiki 1.43.11, PHP
  8.3.35, SQLite, image digest
  `mediawiki@sha256:76b3d47b50c4e4a78f82e814917421cb62c49b0c56b4a0f981cd74e2bddd10ac`
  (identical in both runs and now the harness default), ParserFunctions 1.6.1,
  Cite and Scribunto loaded.
- Two render passes were byte-identical for every page. Revision identities
  recorded: pages 5, 6 and 7 (revisions 5, 6 and 7); the redirect returns 307
  to the target.
- Observation: `mw:Extension/math` is absent from the Gravity page, as
  expected with the Math extension unconfigured. Math remains unqualified.

## Limits

- Fixture pages are authored for this repository (CC0-1.0). They are imported
  with `edit.php`, not from a public-dump XML file, so this does not yet prove
  that dump-imported revisions plus their template and module dependencies
  render identically.
- Math is only *observed* (`mw:Extension/math` is recorded, never asserted):
  the Math extension and its renderer are not configured.
- The image is pinned by digest; upgrading it requires a new accepted run.
- Output is compared against content expectations, not against
  revision-pinned official Wikimedia output.
- No throughput, CPU or RAM measurements, and no mandatory-media discovery.

## Remaining for SRC-003

Dump-imported dependency reconciliation (templates, modules, file
descriptions), Math rendering, comparison with official output for real
representative articles, and performance recording.
