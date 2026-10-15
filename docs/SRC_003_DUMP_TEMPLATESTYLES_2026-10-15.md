# SRC-003 — Public-dump TemplateStyles renderer correction (2026-10-15)

**Scope:** Narrow repair of the live-dump renderer qualification environment, not an upstream source-authenticity or full product acceptance claim. Canonical SRC-003 remains unchecked.

## Incident and corrective design

The first live public-dump qualification at master `4a1a7926c9523e16a15ea22ebce8de6b10d0ac84` passed SHA-256 publication-member staging and imported roughly 2,000 pages before MediaWiki's importer refused a `sanitized-css` content model. [Renderer dump run 38801394479](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38801394479), job `116651042546`, step 5 failed with `MWUnknownContentModelException`. The harness's MediaWiki 1.43 image did not include TemplateStyles.

TemplateStyles registers `sanitized-css` and is bundled beginning with MediaWiki 1.44. The public dump runner now uses a separate, immutable MediaWiki 1.44-series Docker Hub image-index digest, explicitly loads TemplateStyles, and **fails before importing** unless its `extension.json` exists and the MediaWiki siteinfo API reports a 1.44-series runtime with TemplateStyles registered. The separate synthetic fixture runner retains its previously qualified 1.43 digest.

The harness does not strip CSS-model pages, disable the importer checks, or claim that a syntactically complete import is a faithful rendering of every source revision. The existing article revision/content comparison and artifact upload remain mandatory. The image is source-pinned but a new image is not qualified until exact-head CI succeeds.

## Local verification

- `bash -n tools/renderer-harness/run_dump.sh` — passed.
- `python3 -m unittest scripts/test_filter_dump_for_render.py scripts/test_compare_rendered_html.py -q` — 12 tests passed.
- Actual dockerized import, extension-version report, fidelity results and resource throughput require exact-head `Renderer dump qualification` CI. **Do not check SRC-003 yet.**

## Upstream references

- https://www.mediawiki.org/wiki/Extension:TemplateStyles
- https://www.mediawiki.org/wiki/Bundled_extensions_and_skins
