# ADR: Revision-pinned public-dump rendering fidelity

**Status:** Proposed evaluation; SRC-003 remains unchecked  
**Date:** 2026-10-12  
**Related:** SRC-002, SRC-003, SRC-005, PACK-003, PACK-004, TRUST-001

## Context

The existing current-content fallback discovers checksum-listed XML/bzip2 shards, stages and validates bounded compressed members, and extracts project/page/revision-scoped records as JSONL. Those records preserve wikitext, namespace, title, redirect and source identity; they are **not** rendered HTML. Parsing XML does not expand templates, run Scribunto Lua, render math, resolve reference templates or enumerate every required offline visual. Matching an inventory SHA-256 is a byte-integrity check, not a Wikimedia publisher signature.

The product requires source-matched visible HTML, retrieval blocks, category/redirect metadata and mandatory offline media, with stable citation identity. A current online HTML page cannot be substituted for an older dump revision.

## Decision and pipeline boundaries

1. **Acquisition:** Pin a completed upstream generation, project, source URL set, actual inventory digest, all member digests and publication metadata. Validate complete membership and compressed bytes in private staging. Keep transport-origin verification distinct from cryptographic publisher trust.
2. **Extraction:** Retain raw source and immutable (project, page ID, revision ID) identity. Reject malformed XML, conflicting identities, duplicate revisions, unexpected content models, and inconsistent or incomplete source partitions.
3. **Rendering:** Evaluate a pinned compatible MediaWiki/Parsoid stack, including template pages, Scribunto modules, parser functions, math/cite extensions, localization, site configuration and their coherent revision dependencies. Record the exact renderer build/configuration and dependency snapshot. The renderer must not silently fetch live templates or media outside the pinned source generation.
4. **Normalization:** Convert rendered DOM and revision-consistent metadata into canonical article/section/block/reference/media records. Sanitize HTML and SVG, preserve math semantics, captions, infoboxes, tables, reference anchors and text direction. Require exact source project/page/revision equality before associating HTML and evidence blocks.
5. **Offline publication:** Resolve, attribute and verify all required images, diagrams, maps, equations, fonts, CSS and captions. Build a versioned pack with digest-verified objects and indexes. Only mark a pack offline-ready after complete object verification and independent installed-reader acceptance. Signed publisher origin is a separate trust policy; unsigned verified custom packs must never be called official.

## Candidate rendering strategies

| Candidate | Advantages | Qualification gaps |
| --- | --- | --- |
| Self-hosted revision-pinned MediaWiki/Parsoid with coherent dependency data | Strongest candidate for template/Lua/math/reference fidelity and repeatable builds without mandatory paid API | Need reproducible renderer and extension versions, matching transclusion dependency graph, network isolation, resource measurements and high-fidelity reference fixtures. |
| Revision-matched officially rendered HTML when publicly obtainable | Avoids operating a renderer and provides an independent comparison oracle | Must establish actual coverage, access conditions, exact revision and source identity; cannot assume live/current responses match dump revision. |
| Custom limited wikitext converter | Lightweight for explicitly scoped previews | Cannot establish general Lua/template/parser-function/math/reference parity. Never claim full article fidelity or offline-readiness based on this alone. |

**Provisional choice:** retain raw current-content XML as reproducible source input and evaluate pinned MediaWiki/Parsoid as the preferred rendering reference. If the required coherent transclusion/module data cannot be obtained, reject affected pages or builds rather than silently degrade them to apparently complete offline articles. This ADR is an architecture plan, not a claim that a renderer is implemented or qualified.

## Reproducibility and provenance contract

A normalized article receipt must include source generation, project, page ID, revision ID and timestamp, raw source digest, renderer build/config digest, dependency generation, normalized content digest and required-media manifest. Record renderer failure and unsupported-extension reasons explicitly. Detect redirects by source identity, not just a guessed title; preserve namespace and deletion changes. Do not join independently fetched categories or HTML to an article without a validated coherent source identity.

Retrieval evidence IDs remain tied to the exact canonical revision. Never reuse old evidence IDs for newer visible HTML. Missing mandatory media or incomplete attribution fails the offline-ready gate even if text can be displayed.

## Test and acceptance plan

The fixture matrix must include nested templates and parser functions, Scribunto Lua, named and reused citations, infoboxes, complex tables, MathML/SVG equations, diagrams, maps, images/captions/license metadata, multilingual and right-to-left text, redirect/category cycles, disambiguation, deletion/namespace transitions, schema drift, duplicate page identities, mismatched revisions, hostile HTML/SVG, and deliberately missing dependencies.

For each fixture, record pinned source/renderer/dependency hashes, rendered output, comparison baseline, media inventory and actionable failure diagnostics. Run a deterministic network-isolated CI smoke suite and a representative real-publication sample covering mathematics, physics and non-English articles. Measure processing throughput, CPU, memory, storage and deployment complexity. Distinguish unsupported rendering from acceptable semantic differences; document a threshold before qualification.

SRC-003 acceptance requires: independently verified complete source; revision-matched template/Lua/math/reference rendering; mandatory offline assets and attribution; deterministic canonical records; source-to-pack-to-installed-reader roundtrip; and actual airplane-mode desktop and Android article/search acceptance. These tests must fail closed for missing visuals, mismatched revisions, partial exports and corrupt objects.

## Remaining implementation

Build the pinned renderer harness and transclusion dependency acquisition, evaluate fidelity against exact-revision references, implement DOM/evidence/media normalization, verify complete live source publication and pack generation, and qualify offline desktop/Android installs. No SRC-003 top-level checkbox should change until all gates pass.

## Engineering references

- MediaWiki XML dumps: https://www.mediawiki.org/wiki/Manual:XML_Dumps
- Parsoid: https://www.mediawiki.org/wiki/Parsoid
- Scribunto: https://www.mediawiki.org/wiki/Extension:Scribunto
- Wikimedia dump distribution: https://dumps.wikimedia.org/
