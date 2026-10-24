# SRC-002 / PACK-002 — Mainspace title and alias collision audit

**Date:** 2026-10-23  
**Status:** Qualified partial implementation; neither parent TODO task is complete.

## Change

`graph_from_joined` in `crates/wiki-pack-builder/src/enterprise_graph.rs` now
rejects distinct namespace-0 pages claiming the same canonical title,
including underscores and spaces that denote the same MediaWiki title.
It also rejects a redirect alias conflicting with any canonical mainspace
title, even when the alias record precedes the canonical record, and rejects
different aliases whose underscore/space spellings are equivalent. Alias
collision detection is scoped to namespace 0; legitimate category pages
remain separate identities.

The check is applied to all supplied typed Enterprise records, including
unselected records. This closes a direct DTO bypass of title consistency
without assuming the supplied batch is an authenticated or complete snapshot.
The collision key is intentionally limited to underscore/space equivalence;
it is **not** complete language-dependent MediaWiki title normalization.

## Regression and CI

- First implementation: `b7ea14485e8225875059656aa00a60ab764589b9`; Rust format failed
  ([40070480312](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40070480312)).
- Corrected implementation: `86f740162dbbc1ef5049f2b6144b06f151f290b5`.
- [Rust workspace 40070525490](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40070525490): passed.
- [Platform shells 40070525416](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40070525416): passed.
- Regression: duplicate mainspace canonical titles, underscore/space
  equivalents, alias/canonical collisions regardless of record order,
  alias/alias equivalents, self-alias, and valid category namespace separation.

## Remaining acceptance

Independent Wikimedia publisher provenance and complete joins, full title
normalization, exact category coverage, deterministic authenticated pack
construction, mandatory media, and real-device acceptance remain unproven.
**SRC-002 and PACK-002 remain unchecked.**
