# PACK-002 — Revision-bound joined related-link preview (partial)

Date: 2026-10-16
Status: Implementation pending exact-head CI qualification; PACK-002 remains unchecked.

The shared `wiki-pack-builder::joined_related::preview_joined_selection_with_related` API combines the existing strict Enterprise revision-joined category preview with optional bounded related-link expansion. It accepts typed project/generation/page-ID link identities, a configured hop/new-page/edge budget, and the original topic rules. Every link endpoint, including links outside the selected topic, must be an actual joined namespace-0 page with a known revision. Unresolved redirect aliases and non-mainspace link targets are rejected; callers must supply canonical mainspace link IDs. Explicit exclusions are canonicalized through the already-validated redirect identities. The final selection also respects the topic's total page limit.

The result returns sorted page-ID/revision pairs, a count of related additions and a domain-separated SHA-256 receipt binding the base revision selection, limits, canonical link edges and selected revisions. Reordered equivalent link lists produce the same receipt. Different revisions and changed link identities change the receipt. No incomplete selection is returned on budget exhaustion or malformed links.

**Trust boundary:** Matching project, generation and revision identities proves only local consistency of caller-provided records. It does not independently authenticate Wikimedia, establish complete source coverage, verify visual-media completeness, or create an installable/signed pack. The API explicitly reports `related_source_authenticated: false`. Production PACK-002 still needs authenticated source ingestion, full builder/manifest integration, persistent topic definitions and cross-platform acceptance.

Qualification evidence will be recorded after exact-head CI.
