# SRC-003 — Private staging of a complete native observed census

**Date:** 2026-10-23
**Status:** Qualified implementation slice; SRC-003 remains unchecked.
**Exact qualified source SHA:** `07c9b540918a1a9d15146b0f4f057811eb5fba63`

## Implementation

`stage_pinned_native_bzip2_census` opens every inventory-pinned compressed member in its declared order using a caller-provided read-stream factory. The expected member digest always comes from `PinnedInventory`, not from a callback argument. Every member passes native Rust multistream bzip2 decoding, compressed SHA-256 and EOF, bounded decoded bytes, UTF-8/XML envelope validation, page/revision metadata collection, revision-date checks, shard ordering and namespace-specific observed census export.

The staging root must be private. Census bytes are written to a temporary file and atomically linked into a no-overwrite destination only after every member, page audit and output flush succeeds. A missing source stream, malformed member, bad digest or semantic failure prevents publication and removes the temporary file.

## Qualification

- Exact code SHA: `07c9b540918a1a9d15146b0f4f057811eb5fba63`.
- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39983574941 — success, including rustfmt, strict Clippy and workspace tests.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39983575216 — success.
- Synthetic tests: real bzip2-compressed two-shard MediaWiki XML produces ordered `1\n201\n` namespace-zero IDs, excluding contributor IDs; forged checksum on the *second* shard and a missing source stream both leave no partial census file.
- Prior source commits `486dae6568de9611426b886d49d36201542936e2` and `4c620b6d3b40517b70a64bd9cb71c5b97de07411` failed format and fixture tests respectively; corrected code is the SHA above.

## Limits and remaining gates

This is an **observed** page-ID census derived from the same XML as the imported pages. It cannot independently establish that the publisher released every expected page or that a mirror supplied every authorized shard. Upstream publisher authentication, independent expected-page coverage, full MediaWiki XML compatibility, Parsoid-compatible rendered HTML and evidence-block fidelity, mandatory offline images/diagrams/math/attribution, pack activation, and real desktop/Android acceptance remain outstanding. Do not mark SRC-003 complete or activate a source snapshot based on this stage alone.
