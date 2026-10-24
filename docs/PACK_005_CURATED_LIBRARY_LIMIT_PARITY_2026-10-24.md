# PACK-005 — Curated Library Preview Limit Parity (2026-10-24)

## Scope

The revision-joined `wiki-pack-builder::curated_preview` API previously forced each curated Mathematics, Physics, or Computer Science preview to use its maximum seed traversal depth and page count, even though the CLI already allowed callers to choose tighter limits. This change adds a typed `CuratedLimits` options argument to `preview_curated_topics_with_limits`, retaining `preview_curated_topics` as a backward-compatible default wrapper.

The effective limits are included in the existing selection-rule receipt and therefore in the curated-definition SHA-256 fingerprint. Equivalent reordered/duplicate curated IDs remain deterministic. A depth of zero is permitted; a page limit of zero, a depth above the seed ceiling, or a page limit above the seed ceiling is rejected. A tighter page limit that cannot contain the selected articles produces the existing resolver error rather than silently truncating selection.

## Evidence

- Implementation on `master`: `efc44f065606913f4592684905a5cc934955fdff`.
- Rust workspace (format, strict Clippy, tests): [run 40082557292](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40082557292) — passed.
- Platform shells: [run 40082557325](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40082557325) — running when this document was prepared.
- Prior failed attempts: `cfba500c4579a080694c8ed12a2a6f377f01f630` (strict Clippy argument count); `81a9b6146a83190154b37a326a2ab37ddcb2d725` (rustfmt).
- Synthetic unit regressions in `crates/wiki-pack-builder/src/curated_preview.rs` cover default compatibility, stricter effective bounds, fingerprint changes, deterministic topic ordering, and zero/over-ceiling rejection.

## Qualification boundary

This is local library/CLI policy parity only. The caller-supplied Enterprise records are not independently publisher-authenticated, complete category fan-out is unmeasured, no complete offline media pack is constructed, and neither desktop nor physical Android acceptance is established. **PACK-005 stays unchecked** until the canonical acceptance criteria are satisfied.
