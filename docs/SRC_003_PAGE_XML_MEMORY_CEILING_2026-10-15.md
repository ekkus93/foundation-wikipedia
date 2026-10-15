# SRC-003 — per-page diagnostic XML memory ceiling (2026-10-15)

**Status:** Partial resilience gate only; SRC-003 remains unchecked.

The public-dump diagnostic filter buffered every XML `<page>` record in a
Python list. Previously only a *sampled* article received the 64-MiB XML
ceiling, and that check occurred **after** the entire block had been retained.
A huge non-sampled Template/Module page could bypass that guard. The parser
now counts UTF-8 bytes per page as lines arrive and rejects any page whose
buffer would exceed 64 MiB, before appending the over-budget line. This
applies equally to sampled articles, template/module dependencies and pages
otherwise destined to be dropped. The byte cap is injectable for deterministic
small-fixture regression tests.

The byte limit is a conservative diagnostic importer bound. If a legitimate
official source includes a larger single page, this diagnostic intentionally
fails closed rather than silently truncating or ignoring it. A portable
streaming Rust production importer needs its own measured policy, as does
independent source publication completeness. The parser still necessarily
holds a single bounded page block and does not cap a single incoming text line
*before* Python receives it. This does not provide images, Commons attribution,
offline HTML/CSS/font resolution or a qualified complete source fallback.

Exact-head Rust, Platform and renderer CI are required before calling this
particular byte-budget improvement qualified. All overarching TODO items
remain unchecked.

## Exact-head qualification

The per-page XML-byte budget and fail-closed regression landed directly on
`master` at `c90ec58d2e3d0ecef1fc6ecff79c961b3a1230f3`. Ralph Bridge
operation `79301bdf5c3d4493a0ebaceb0e625771` accepted and post-write
verified the precise head and three changed paths (`write_reconciled=false`).
All matching workflows **passed**:
[Rust 38820735877](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38820735877),
[Platform 38820735875](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38820735875),
[Renderer 38820736053](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38820736053).
This proves the scoped diagnostic guard compiled and qualified, not that no
large source page will ever trigger the fail-closed cap.
