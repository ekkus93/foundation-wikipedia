# SRC-003 — MediaWiki/Parsoid renderer fixture harness (2026-10-15)

**Status:** harness implemented; not yet qualified. Canonical TODO `SRC-003`
remains unchecked. This implements the "version-pinned MediaWiki/Parsoid
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

## Limits

- Fixture pages are authored for this repository (CC0-1.0). They are imported
  with `edit.php`, not from a public-dump XML file, so this does not yet prove
  that dump-imported revisions plus their template and module dependencies
  render identically.
- Math is only *observed* (`mw:Extension/math` is recorded, never asserted):
  the Math extension and its renderer are not configured.
- `mediawiki:1.43` is a floating tag; the resolved digest is recorded per run
  in `environment.txt` and should be pinned once a run is accepted.
- Output is compared against content expectations, not against
  revision-pinned official Wikimedia output.
- No throughput, CPU or RAM measurements, and no mandatory-media discovery.

## Remaining for SRC-003

Dump-imported dependency reconciliation (templates, modules, file
descriptions), Math rendering, comparison with official output for real
representative articles, pinning the image digest, and performance recording.
