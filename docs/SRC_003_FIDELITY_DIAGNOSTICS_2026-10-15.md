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
