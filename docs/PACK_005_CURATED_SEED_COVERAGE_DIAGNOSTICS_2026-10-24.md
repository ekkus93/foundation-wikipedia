# PACK-005 — Curated Seed Coverage and Overlap Diagnostics (2026-10-24)

## Implementation

The provisional `wiki-pack preview-topic` CLI now measures each requested curated Mathematics, Physics or Computer Science seed independently against the same supplied category graph, revision-label claim, depth/page policy, exclusions, redirects and disambiguation flags. It reports per-seed selected-page count, visited categories, raw article/child mentions and the largest category. The composite report includes unique seed-selected pages and overlap *mentions* (the sum of individual seed counts minus the size of their union). A page selected by three seeds contributes two overlap mentions.

Explicit includes and additional custom roots are **not** attributed to any curated seed; the non-seed count is the difference between the final selection (including optional related expansion) and the union of seed-only selections. This prevents operator overrides from inflating seed coverage measurements. Requested seed IDs are sorted and deduplicated before measuring, so their order and repetition do not alter counts. Custom-only previews retain their previous output shape.

Both human-readable and JSON preview output include the new diagnostics. JSON fields are `curated_topic_coverage`, `curated_unique_seed_page_count`, `curated_overlap_page_mentions` and `curated_non_seed_selected_page_count`. All existing `source_authenticated=false`, `publisher_signature_verified=false`, `offline_ready=false` and `pack_installed=false` disclaimers remain unchanged.

## Verification

- Qualified code SHA: `85e5b4a607daf65a5d960a8c499885f3a5c07df8`.
- [Rust workspace #40086542530](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40086542530): passed exact-head format, strict Clippy and tests.
- [Platform shells #40086542554](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40086542554): passed exact-head desktop and Android shell workflows.
- Initial source SHA `c209dd8bcb966d3b651b4f727850439952a8d7ca` failed rustfmt in [Rust #40086460543](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40086460543); corrected source passed at the qualified SHA.
- Unit regression in `tools/wiki-pack/src/preview_topic.rs` verifies shared membership, excluded pages, explicit additions, deterministic seed ordering/deduplication and empty curated selection behavior.

## Remaining qualification

The CLI operates on a **caller-supplied, untrusted local category graph**. It cannot establish that Wikimedia published, authenticated or completed that graph. Real-source fan-out/coverage benchmarking, complete reproducible topic builds, required offline media, installation, and physical-device acceptance remain outstanding. **PACK-005 is not complete.**
