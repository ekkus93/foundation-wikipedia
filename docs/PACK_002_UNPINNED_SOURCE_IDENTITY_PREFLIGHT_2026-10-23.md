# PACK-002: Source identity preflight for unpinned topic previews

**Date:** 2026-10-23  
**Scope:** Partial raw resolver consistency validation.

## Change

`wiki-pack-builder::selection::resolve_with_redirects` and its
`resolve` wrapper now enforce the same full-input page/redirect/
disambiguation identity checks previously applied only by
`resolve_pinned`. A malformed unselected category record, unreachable
redirect cycle, invalid redirect target or disambiguation ID cannot
produce a superficially valid unpinned preview.

The common `validate_source_identities` preflight scans all supplied
article IDs, redirect endpoints and disambiguation IDs, then checks
all redirect chains with the existing bounded suffix-depth algorithm.
`resolve_pinned` delegates to the shared traversal rather than scanning
the same input twice.

## Tests and qualification

- Code SHA: `f1eb0253cb12e507e1a15909abb3a66fc4ef13be`.
- Synthetic tests exercise invalid unselected page IDs, a positive
  repaired graph, an unreachable two-node redirect cycle, a zero redirect
  target, and an out-of-range disambiguation ID.
- [Rust workspace CI 40047664316](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40047664316):
  passed formatting, strict Clippy and workspace tests.
- [Platform shells CI 40047664509](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40047664509):
  running at documentation; recheck terminal result.

## Remaining

Caller-provided graph identity validation does not authenticate
Wikimedia source data, establish complete category coverage, implement
media-complete pack construction, or qualify desktop/Android installation.
PACK-002 remains unchecked.
