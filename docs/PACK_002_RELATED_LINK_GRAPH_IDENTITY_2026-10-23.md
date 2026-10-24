# PACK-002: Validate raw related-link graph identities

**Date:** 2026-10-23  
**Scope:** Partial optional linked-prerequisite selection hardening.

## Implementation

The public `expand_related_links` function in
`crates/wiki-pack-builder/src/related.rs` now validates every source and
destination page ID in its caller-supplied adjacency map before BFS. Previously,
invalid page IDs in unreachable components or edges beyond the hop budget could
be silently ignored, allowing a valid-looking preview over malformed supplied
source metadata. IDs must be positive and fit signed SQLite's `i64` range.

The revision-joined wrapper already checks its entire supplied link inventory.
This change applies the same fail-closed principle to the raw direct-call API.

## Synthetic regression

- Unselected adjacency key `0` is rejected.
- Unselected adjacency target `i64::MAX + 1` is rejected.
- Unselected adjacency key `i64::MAX + 1` is rejected.
- A malformed target one hop beyond the expansion limit is rejected.
- Valid graph with one-hop selection continues to return `[1, 2]`.
- Existing cycle, exclusion, sorted-selection and edge-budget tests remain.

Code progression:
- `d1271b54eaa24465e986fbf0aaa1d00810d0d545`: first implementation; rustfmt failed.
- `3588e205c8ee6e9f4aa829d9dd833c456036898f`: formatting repair; file-tail truncation detected by Rust CI.
- `7c48f6c2fdff5c6bda754733b43d8c49d1c412ea`: restored test module terminators.

CI at documentation: [Rust workspace 40055313022](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40055313022) and [Platform shells 40055313028](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40055313028) were running; check their terminal status.

## Still outstanding

A syntactically consistent caller-supplied link graph is not authenticated
Wikimedia provenance or proof of complete topic coverage. PACK-002, pack
construction, mandatory media, and device acceptance remain incomplete.
