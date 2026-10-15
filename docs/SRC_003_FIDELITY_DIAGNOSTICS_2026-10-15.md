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
