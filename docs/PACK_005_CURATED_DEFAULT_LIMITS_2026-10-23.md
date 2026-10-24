# PACK-005: Default curated preview limits (partial)

**Date:** 2026-10-23  
**Qualified source SHA:** `c8c11ea30c46cb39fad542ca8be5f12d6beeb4c0`

## Implemented

`wiki-pack preview-topic [--json] <local-category-graph.json>` now permits
omitting `depth_limit` and/or `page_limit` when `curated_topics` is
nonempty. The CLI derives missing limits from the selected, editable
Mathematics/Physics/Computer Science definitions. Explicit tighter limits
remain supported. Any explicit value exceeding a curated seed's cap fails
closed, and incompatible composite seed policies are rejected.

Custom previews without `curated_topics` must continue to specify both
limits; omission is rejected rather than selecting an implicit unbounded
policy. Effective limits are included in JSON and human-readable previews.
No source fetch, pack build, signature check, or install occurs.

## Tests and CI

- Curated-only JSON fixture omits explicit roots and both limits and
  succeeds using the `mathematics` seed.
- Direct parser tests cover defaults, explicit stricter values, invalid
  generation labels and custom missing-limit rejection.
- Initial SHA `4c41a5523a666b5efe3d0030c356f55df5978117`
  failed [Rust workspace 40068221524](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40068221524)
  at rustfmt; formatting repair
  `c8c11ea30c46cb39fad542ca8be5f12d6beeb4c0` passed
  [Rust workspace 40068264905](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40068264905)
  and [Platform shells 40068264907](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40068264907).

## Remaining

The preview remains a local, caller-supplied category-graph diagnostic.
Publisher-authenticated generation and complete category coverage, real
fan-out and size benchmarks, mandatory offline media, pack construction,
and desktop/Android acceptance are not demonstrated. **PACK-005 stays unchecked.**
