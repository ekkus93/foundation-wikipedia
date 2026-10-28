# SRC-002 — Enterprise batch publication identity

**Date:** 2026-10-27 (America/Los_Angeles)  
**Scope:** Partial typed Enterprise bulk-join consistency; not complete SRC-002  
**Qualified code SHA:** `c105a69f1622b147941c6ef0edceacfb12fe55de`

## Problem

`join_enterprise_batch` previously enforced exact structured/regular project, page ID, revision, generation, language and namespace **within each page pair**. It did not enforce one project and one generation **across the entire returned batch**. A caller could construct two individually matching page pairs from different Wikimedia projects or source generations and receive a successful combined batch. PageKey-only deletion markers from a different project could also be silently accepted if their IDs did not collide.

A bulk import's consistent publication identity is required for revision-pinned source provenance, category graph construction and deterministic pack selection. This is distinct from independently authenticating a source publication.

## Change

In `crates/wiki-source/src/enterprise.rs`:

- A shared batch-project guard checks every tombstone, structured page and regular companion; it returns `EnterpriseError::MixedProject(PageKey)` for foreign project IDs.
- A batch-generation guard validates syntax and requires the same generation ID for every structured and regular record; it returns `InvalidGeneration` or `GenerationMismatch` as appropriate.
- Existing duplicate page, duplicate revision, tombstone/live conflict, per-pair revision and identity checks remain in place.
- The legacy `Vec<PageKey>` tombstone API cannot prove generation identity. For generation-scoped deletion evidence, use `join_enterprise_ndjson_scoped` with `DeletionMarker`; this change does not fabricate missing provenance.

## Synthetic regression and CI

`enterprise_batch_rejects_mixed_projects_generations_and_tombstone_sources` exercises a valid two-page same-publication batch, a second project with matching per-page structured/regular identities, a second generation with matching per-page identities, a forged regular generation, foreign tombstones in live and tombstone-only batches, and an invalid typed generation. These are local fixture checks; no real publisher feed was authenticated.

- Initial source SHA `9b6c62473413aeab4132fe756d07b9dedc6a6424` failed formatting [Rust 40629321205](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40629321205).
- Corrected source SHA `c105a69f1622b147941c6ef0edceacfb12fe55de` passed [Rust workspace 40629413328](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40629413328) (format, Clippy and workspace tests) and [Platform shells 40629413322](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40629413322).

## Remaining acceptance

**SRC-002 remains unchecked.** A consistent typed batch is not proof that an upstream generation was completed, publisher-authenticated, exhaustive for page/category/redirect memberships, rendered with all required visual resources, or safely installed on desktop/Android. The source importer still requires authoritative publication and completeness gates and real-device qualification.
