# PACK-002: Unique-category BFS queue and bounded raw edges

**Date:** 2026-10-23  
**Scope:** Partial deterministic topic resolver safety and correctness.

## Change

The shared `wiki-pack-builder::selection::resolve_with_redirects` traversal
now deduplicates category identities when enqueuing them. The breadth-first
queue visits each category at its minimum root-relative depth, even if a
snapshot contains repeated roots, duplicate child links, cycles or diamond
edges. Distinct categories are limited by the existing budget derived from
the selected page limit (minimum 1,024; maximum 100,000).

A separate raw-edge inspection budget, 64 times the distinct-category budget
capped at 1,000,000, limits work caused by excessive duplicate root and child
references. This preserves fail-closed resource limits without rejecting a
small unique category graph merely because its links repeat.

The graph digest and definition digest contracts are unchanged. Selection
continues to be sorted by canonical page ID, with bounded warnings.

## Regression and CI

- Code SHA: `0f7f445b6b9797e40d56ba0153ea45a257798ebc`.
- 2,048 duplicate roots, 4,096 duplicate root-to-leaf edges and 2,048
  leaf-to-root cycle edges resolve to the same page IDs as a minimal graph.
- More than 1,024 unique categories with the minimum budget fail closed.
- 65,537 duplicate raw roots exceed the independent raw-edge budget and fail.
- Initial code SHA `a6913e6bdcb3c3ded5492c999fa74c9140f9e7bd`
  failed [Rust formatting CI 40047273218](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40047273218);
  the formatting correction was committed directly to master.
- [Rust workspace CI 40047328427](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40047328427)
  passed format, strict Clippy and tests.
- [Platform shells CI 40047328314](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40047328314)
  was still in progress at documentation time; verify terminal result.

## Not yet qualified

This improvement does not establish independently authenticated and complete
Wikimedia category membership, production pack selection review, media
coverage, signed pack export/install, or desktop/Android real-device
acceptance. PACK-002 remains unchecked.
