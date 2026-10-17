# PACK-002 — Revision-bound joined related-link preview (partial)

Date: 2026-10-16
Status: Qualified shared-library regression slice; PACK-002 remains unchecked.

The shared `wiki-pack-builder::joined_related::preview_joined_selection_with_related` API combines the existing strict Enterprise revision-joined category preview with optional bounded related-link expansion. It accepts typed project/generation/page-ID link identities, a configured hop/new-page/edge budget, and the original topic rules. Every link endpoint, including links outside the selected topic, must be an actual joined namespace-0 page with a known revision. Unresolved redirect aliases and non-mainspace link targets are rejected; callers must supply canonical mainspace link IDs. Explicit exclusions are canonicalized through the already-validated redirect identities. The final selection also respects the topic's total page limit.

The result returns sorted page-ID/revision pairs, a count of related additions and a domain-separated SHA-256 receipt binding the base revision selection, limits, canonical link edges and selected revisions. Reordered equivalent link lists produce the same receipt. Different revisions and changed link identities change the receipt. No incomplete selection is returned on budget exhaustion or malformed links.

**Trust boundary:** Matching project, generation and revision identities proves only local consistency of caller-provided records. It does not independently authenticate Wikimedia, establish complete source coverage, verify visual-media completeness, or create an installable/signed pack. The API explicitly reports `related_source_authenticated: false`. Production PACK-002 still needs authenticated source ingestion, full builder/manifest integration, persistent topic definitions and cross-platform acceptance.

Qualification: commit `1bfb5c82936eee407f630636e72e576b4e970e5d` passed exact-head Rust CI 38987143473 and Platform CI 38987143469. Intermediate rustfmt failure in run 38987096380 was corrected. This does not establish real-source, offline-media or Android acceptance.

Additional qualification: the public joined-preview API now has an end-to-end synthetic joined-record regression for exact revisions, deterministic link order, excluded related pages, namespace rejection and the combined page budget. An unselected-link inventory cap of 1,000,000 records prevents unbounded canonical edge allocation. Exact-head `6d11e179d38a212bb824138ead09fbbb0dbbdf00` passed Rust CI 38987480502 and Platform CI 38987480477. Intermediate Rust formatting failure (run 38987442761, job 117262085429, step 4) was corrected. These tests are not real Wikimedia-source or Android acceptance.

Revision-identity hardening: each typed related link now requires `from_revision_id` equal to its joined source article revision. Stale or zero revision identities fail closed. Exact-head commit `90a1f6425ef0eb129d05b3c41c6baf2738ef9ae3` passed Rust CI 38987744247 and Platform CI 38987744223. Publisher authenticity remains unverified.
