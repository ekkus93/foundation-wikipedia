# PACK-002 — Category fan-out preview diagnostics

**Date:** 2026-10-23  
**Status:** Partial, scoped synthetic qualification. PACK-002 remains unchecked.

## Behavior

`wiki-pack-builder::selection::resolve_with_redirects` now adds
deterministic preview warnings when a visited, nonadministrative
category has more than 32 immediate child links or more than 512
direct article memberships. The diagnostic includes the category
label and the raw count. It is independent of whether recursion
depth permits visiting those children, so the preview can warn
about broad categories even when depth is capped.

Warnings are collected in an ordered set, as with the existing
depth-limit, redirect and administrative-category warnings.
Thresholds are diagnostic, not new hard limits: the selected
page-ID set, canonical redirect handling and existing candidate
budgets are unchanged.

## Regression and CI

- `warns_on_unusually_broad_category_fanout_without_changing_selection`
  exercises exact thresholds (32/512), first exceedances (33/513),
  stable warning ordering, and pinned/unpinned result parity.
- Implementation: `b31ee238f4ba955735ca7818f7d6227c5671d026`.
- Rustfmt repair: `e1b573b275cdbaeefbe2c07326b8433b5af67df2`.
- [Rust workspace CI 40046583038](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40046583038):
  passed formatting, strict Clippy and workspace tests.
- [Platform shells CI 40046583039](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40046583039):
  running when documented; verify terminal result separately.

## Unqualified

The graph remains caller-supplied; publisher authentication, exact
source cutoff and category completeness, real Pack Builder review UI,
selection/media size estimation, downloadable pack generation, and
physical desktop/Android acceptance are not demonstrated.
