# SRC-003 — Bounded per-page renderer fidelity diagnostics (2026-10-15)

**Status:** Partial observability improvement only. The canonical `SRC-003` task remains unchecked.

## Motivation

The previous exact-head real-dump renderer run [38804790747](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38804790747) imported over 3,000 pages but reported mean link Jaccard `0.3737004202811399` and **one local error page** across 25 sampled pages. Aggregate counts alone were insufficient to identify affected page IDs or determine which links diverged.

## Changes

- The comparison report now counts per-page missing/extra internal links and retains at most ten stable sorted examples of each kind, not the complete link sets.
- Its aggregate identifies up to ten page titles, original source revision IDs and error-marker categories for local renders containing error markers, plus the number of pages below `0.5` measured link overlap.
- During CI, the diagnostic command prints at most 50 bounded `FIDELITY-WARNING` records for problem pages. The full JSON report remains attached to the workflow's existing CI artifact.
- The existing behavior that distinguishes operational import/fetch success from *true* rendering-fidelity qualification is intentionally preserved. A green workflow does **not** certify revision-fidelity or offline visual completeness, and no arbitrary link-score threshold is treated as an acceptance criterion.
- Regression tests cover counts, sorted bounded mismatch examples and identification of a local-error page by source revision.

## Local validation

The test-only comparator and filter suite was executed against the user's uploaded `master` snapshot, whose comparator matched the live source code; `8` comparison tests and `6` dump-filter tests passed, and both Python source files compiled successfully.

## Remaining acceptance

Inspect new exact-head CI artifacts/logs to identify the local-error page and whether link differences arise from missing pinned template/module dependencies, variant URI normalization, inconsistent source revisions or true rendering gaps. Do not mark SRC-003 complete before resolving those gaps and qualifying required media and provenance.

## Qualified diagnostics and root-cause separation

The initial detailed-observability implementation at exact SHA `47766b3ebec084327c071bed34c1e7523c4d2683` passed all three workflows: [Rust 38807904595](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38807904595), [Platform 38807904786](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38807904786), [real dump renderer 38807904675](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38807904675). Local tests also passed.

The renderer log pinpointed the only local parser/Lua-error page as **`Wiht (īeg)`**, original source revision `204753`, with five `Lua error` markers and four HTML `class="error"` markers. The source stream is official and checksummed; the running local MediaWiki environment still lacks some revision-consistent Lua/template dependency or site configuration. This needs a separate fidelity fix, not silent exclusion.

The low link score was largely a **metric identity artifact**: links like `./Asia?action=edit&redlink=1` in the incompletely imported local wiki were compared with `./Asia` in live HTML as two different *article targets*. Those point to the same title; only existence/redlink state differs. The follow-up comparator change now normalizes only the precisely recognized `action=edit&redlink=1` suffix into a shared page target; it separately counts mismatched redlink status, rather than erasing that important indication of incomplete source import. Other unknown query strings and section anchors are excluded rather than silently conflated. Local parser errors retain a bounded text excerpt (at most 180 characters) so subsequent CI runs can locate the missing Lua dependency.

**Acceptance limit:** Normalized link equality is not full offline content completeness or actual live revision fidelity. The local wiki deliberately contains only a subset of pages. The real remaining error and mandatory media/attribution checks must still be resolved, and `SRC-003` remains unchecked.

## CI status and scope after metric normalization

The target-normalization commit `6ed7aea0f1e0561e74f912bd071c0c0da0f337d7` passed [Platform shells CI 38808470243](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38808470243) and [Renderer dump CI 38808470283](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38808470283), but unrelated Rust [run 38808470226](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38808470226) failed in a legacy curl subprocess test with Linux `ExecutableFileBusy` / errno 26. This is not a renderer-fidelity failure; the Rust head should not be described as fully qualified until the separately documented spawn recovery and exact-head CI succeed.

The metric correction still distinguishes redlink-status differences from article link identity. The Lua-error page `Wiht (īeg)`, source revision `204753`, remains an independent source-render fidelity gap; the next renderer run will provide a bounded error excerpt to narrow its cause.

## Measured corrected link identity and outstanding Lua fidelity

The exact-head normalized comparison at `6ed7aea0f1e0561e74f912bd071c0c0da0f337d7` passed [renderer dump 38808470283](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38808470283). Its aggregate across 25 samples is mean heading similarity `1.0`, mean **normalized page-target** link Jaccard `0.9715623796617847`, zero pages below `0.5`, 25 pages with redlink-status drift, one page with local errors, no ref/table-count differences. This demonstrates that most of the previously measured link divergence was due to query suffixes on red links in an intentionally subsetted local wiki. It **does not** prove imported/installed link destinations exist offline.

The remaining real Lua error is in the sampled article `Wiht (īeg)`, source revision `204753`: `Module:Convert` line 2164 tries to index nil local `in_current`. That error must be investigated against the exact module/template revisions and MediaWiki/Scribunto configuration. No source page was dropped or a faulty Lua expansion silently accepted as qualified. Subsequent correction of the unrelated curl ETXTBSY test flake passed [Rust 38809250088](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38809250088) and [Platform 38809250425](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38809250425) at SHA `af5d3e15fb16579b979442377755ce669d9f618d`.

**SRC-003 remains unchecked** pending full source/completeness independence, revision/dependency reconciliation, fidelity and mandatory media.

## Upstream-error attribution refinement

An independent public view of the live Anglish Wikipedia article `Wiht (īeg)` also displays `Lua error in Module:Convert at line 2164: attempt to index local 'in_current' (a nil value)`. This strongly suggests that the matching local message may originate in upstream wiki content or Lua module behavior, rather than being caused solely by the dump-import harness. It **does not yet establish exact-revision equivalence**, because the index view can reflect a different source revision or dependency state.

To avoid misclassifying an upstream rendering error as a new local regression, the comparator now records the live page's error-marker types and bounded excerpt alongside the local ones. It emits marker-by-marker local-minus-live deltas, the number of pages with upstream errors, local-only errors and mismatched marker counts. The existing artifact and bounded CI warning include this context. Matching error-marker signatures alone cannot prove rendered text equivalence or required offline-media completeness.

Local tests: 12 renderer-comparison tests and 6 dump-filter tests passed. Exact-head CI is still required before claiming qualification.

Public reference: https://ang.wikipedia.org/wiki/Wiht_(%C4%ABeg) (live/wiki content can change).

## Exact-head upstream-error comparison qualified and revision identity under test

At `dcc8ac371d29c50a5c9dda6061e520b8dd165d77`, [Rust 38809883530](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38809883530), [Platform 38809883541](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38809883541), and [Renderer dump 38809883538](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38809883538) passed. Rendered `Wiht (īeg)` source revision `204753` had exactly matching local/live error-marker counts (five `Lua error` and four `class="error"` each), and the same `Module:Convert` nil `in_current` text. Across 25 pages, live-only/local-only error differences and marker-count drifts were zero. This supersedes attributing that Lua defect solely to local rendering; identical errors do not make the page acceptable for all release goals.

The next observation records each local imported page ID, revision ID and revision timestamp against those of the checksum-pinned source dump sample, separately from HTML similarity. Regression tests mock successful matching and a mismatched local revision. These initial mismatch counters do not yet claim a production revision-qualified public importer, independent source completeness or offline media.

## Source-original wikitext digest recorded before local import

The pinned source-dump filter now hashes the **decoded, original** UTF-8 wikitext of each sampled revision using SHA-256, storing only the hexadecimal digest in the sample manifest (`source_wikitext_sha256`). It also rejects a sampled page whose `<text bytes="...">` metadata disagrees with its decoded UTF-8 byte length, or whose standalone page XML is malformed or greater than the explicit 64-MiB decoding ceiling. No article text is logged. Tests exercise Unicode plus escaped XML markup and intentionally incorrect byte metadata.

This creates the source-side half of a content-fidelity audit for imports that remap local database revision IDs. A later local-raw-content check is still needed to compare the original digest with the destination wiki; matching timestamps alone are not sufficient. **This digest inherits source publication authenticity from the verified upstream member; a hash cannot independently prove Wikimedia provenance or total source completeness.**

The previous explicit source/local ID mapping commit `c31a6aab5af1a3349870e1958021a7132f538715` passed exact-head [Rust 38811336513](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38811336513), [Platform 38811336493](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38811336493), and [Renderer 38811336370](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38811336370). Subsequent source digest checks require their own exact-head CI before being considered qualified.
