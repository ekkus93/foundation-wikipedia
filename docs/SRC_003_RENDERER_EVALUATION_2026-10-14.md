# SRC-003 — public-dump renderer evaluation (2026-10-14)

**Status:** design/feasibility review only; no end-to-end renderer qualified.
Canonical TODO `SRC-003` remains unchecked.

## Official source contract

Wikimedia's MediaWiki Content File Exports documentation describes the
`mediawiki_content_current` dataset as monthly unparsed content for current
revisions. It identifies the presence of the generation's `SHA256SUMS` as
a publication-completion signal and directs consumers to verify every member
against that inventory. This is integrity evidence, not independent
cryptographic publisher-signature authentication or page-census completeness.

Source: https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports

## Rendering alternatives

1. **Current MediaWiki with PHP Parsoid:** Parsoid is bundled with MediaWiki
   since 1.35. A compatible installation must have the relevant wiki's
   templates, modules, extensions, namespaces, and configuration available.
   Render via a supported MediaWiki REST HTML endpoint; do not depend on the
   deprecated internal Parsoid API. Compare exact page/revision identity and
   resulting rendered structure with independent reference output.
   Source: https://www.mediawiki.org/wiki/Parsoid
2. **Scribunto/Lua:** MediaWiki's Scribunto extension executes Lua modules
   invoked through `#invoke`. A parser without the actual module and
   template dependencies is not equivalent for infoboxes and other common
   Wikipedia content. Source:
   https://www.mediawiki.org/wiki/Extension:Scribunto
3. **MediaWiki `renderDump.php`:** Official documentation explicitly says
   this script is unsuitable for publishing/offline use and loads templates
   from the *local wiki database*, not the input XML dump. It is appropriate
   only as a comparative diagnostic. Source:
   https://www.mediawiki.org/wiki/Manual:renderDump.php
4. **Wikitext-as-HTML or regex-only conversion:** rejected as a qualified
   fallback; it cannot faithfully expand templates, Lua, math and references.

## Proposed qualification implementation

- Build a **version-pinned MediaWiki/Parsoid fixture service** with explicit
  parser, extension and dependency-version provenance. It must not require
  an Enterprise subscription. Keep service deployment separate from the
  portable Rust core and do not bundle it with Android.
- Acquire and reconcile exact wiki-generation dependencies: article revisions,
  templates, modules, file descriptions and rendering configuration. Do not
  silently substitute current live templates when rendering an older dump.
- Run representative math, reference, infobox, Lua, table, non-English and
  redirect fixtures through the renderer; compare against revision-pinned
  official output, including mandatory media references.
- Capture HTML/MathML/SVG dependencies, attribution and content hashes before
  pack export. Treat missing expansion dependencies as hard failures, not
  success with degraded text.
- Record CPU/RAM/storage, throughput and repeatability, and make
  snapshot-completeness verification independent of file checksum validation.

## Decision and limitations

Prefer a version-pinned MediaWiki/PHP-Parsoid service as the **candidate** for
public-dump rendering. This is not a qualification decision until the actual
fixture corpus and revision/dependency checks pass. The existing Rust
public-dump modules validate inventories, shard order and page metadata but
do not themselves decompress/parse and render the full XML/wikitext corpus.
No full article or offline-pack acceptance is claimed by this note.
