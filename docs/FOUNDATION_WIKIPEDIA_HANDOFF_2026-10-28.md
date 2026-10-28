# Foundation Wikipedia — Project Handoff (2026-10-28)

**Repository:** https://github.com/ekkus93/foundation-wikipedia  
**Branch:** master (direct exact-head commits, no per-task branches/PRs)  
**Canonical checklist:** [WIKIPEDIA_AI_READER_TODO.md](WIKIPEDIA_AI_READER_TODO.md)  
**Specification:** [WIKIPEDIA_AI_READER_SPEC.md](WIKIPEDIA_AI_READER_SPEC.md)  
**Source snapshot SHA before this handoff-only commit:** 3d06631f1c1c3bc1edb54608dd51f8795fa5ebc1  
**Date:** 2026-10-28, America/Los_Angeles  
**Overall status:** Active implementation, NOT release-ready.

## Executive status and measured completion

- **All explicit TODO checkboxes, including subtasks and future ideas:** 34 / 233 = **14.6%** completed; 199 open.
- **Top-level requirement IDs, including future ideas:** 8 / 77 = **10.4%** completed; 69 open.
- **v0.1 top-level requirement IDs (excluding six deferred FUT items):** 8 / 71 = **11.3%** completed; 63 open.
- **v0.1 checkboxes excluding FUT items:** 34 / 227 = **15.0%** completed; 193 open.
- **Fully completed top-level IDs:** BOOT-001, BOOT-002, BOOT-003, MOD-001, MOD-002, MOD-003, SRC-001, AI-001.
- **Method:** Counted the actual checkboxes in the COMPLETE 1938-line canonical TODO at the source snapshot SHA. Progress log paragraphs, implementation commits and test evidence under unchecked IDs were NOT counted as completions. These are conservative acceptance percentages, not a developer-effort estimate or a claim that remaining IDs have no code.

## Completion by phase

| Phase | Completed top-level IDs | Completed checkboxes (with subtasks) |
| --- | ---: | ---: |
| Phase 0 — Repository/bootstrap, build and quality gates | 3/3 | 12/12 |
| Phase 1 — Canonical model and provenance | 3/3 | 12/12 |
| Phase 2 — Wikimedia acquisition and revision-matched normalization | 1/5 | 7/19 |
| Phase 3 — Immutable snapshot and shared object storage | 0/4 | 0/12 |
| Phase 4 — Pack format, resolver, builder and mandatory media | 0/6 | 0/22 |
| Phase 5 — Publisher signatures, custom trust and hostile import protection | 0/3 | 0/10 |
| Phase 6 — RAG and evidence-based answer contracts | 0/4 | 0/13 |
| Phase 7 — Desktop Wikipedia-like reader | 0/3 | 0/9 |
| Phase 8 — AI provider adapters, streaming and model profiles | 1/5 | 3/15 |
| Phase 9 — M3 Expressive action hub and desktop chat | 0/4 | 0/11 |
| Phase 10 — Desktop Pack Builder screens and CLI parity | 0/5 | 0/12 |
| Phase 11 — Persistent state and multi-page Settings | 0/3 | 0/9 |
| Phase 12 — Android Rust/UniFFI and Compose/WebView reader | 0/4 | 0/12 |
| Phase 13 — Android official/custom pack management and offline media | 0/3 | 0/9 |
| Phase 14 — Optional local Android LLM, quality gate | 0/3 | 0/9 |
| Phase 15 — Official pack publication and update distribution | 0/4 | 0/12 |
| Phase 16 — Security, licensing and privacy | 0/3 | 0/9 |
| Phase 17 — Qualifying UX, reliability and performance | 0/3 | 0/9 |
| Phase 18 — Packaging and end-to-end v0.1 release | 0/3 | 0/11 |
| Deferred — explicitly not v0.1 blockers | 0/6 | 0/6 |

## Last completed development slice: STORE-004 staged FTS5 integrity

- Latest completed change: SQLite FTS5 native integrity-check on a private writable **staged** derived search index, performed AFTER canonical catalog/article/redirect integrity verification and BEFORE atomic publication. An intentionally damaged docsize shadow entry is detected even when ordinary search and stored text remain plausible; failed staging preserves the previously published index.
- Corrected implementation SHA: ac18ff2b416f3a5c431128486d83d3667dd4925b. An earlier formatting failure at 532c5c3ee0ceb79ec2e011c870cb7cfcf49850ed was fixed.
- Latest source/documentation CI snapshot 3d06631f1c1c3bc1edb54608dd51f8795fa5ebc1: [Rust workspace 40783023013](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40783023013) PASSED; [Platform shells 40783023002](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40783023002) PASSED.
- Scope note: [STORE_004_NATIVE_FTS5_INTEGRITY_CHECK_2026-10-28.md](STORE_004_NATIVE_FTS5_INTEGRITY_CHECK_2026-10-28.md); related [verified published FTS open](STORE_004_VERIFIED_PUBLISHED_FTS_OPEN_2026-10-28.md), [canonical schema](STORE_004_CANONICAL_FTS_SCHEMA_IDENTITY_2026-10-28.md), and [redirect-body audit](STORE_004_EXACT_REDIRECT_SUFFIX_AUDIT_2026-10-28.md).
- **Important remaining limitation:** The published index is opened read-only, so its verified opener checks schema plus complete catalog/text/redirect membership but has NOT independently run native FTS5 integrity-check against persisted inverted postings and shadow data. STORE-004 remains unchecked. No new unverified source code was started for this separate future slice; the most recent coherent software increment is complete and green.

## What exists today (partial implementation is not full acceptance)

- Rust workspace/core, article and citation identity models, codec, source manifest checking, desktop/Android build scaffolds and exact-head CI: BOOT/MOD/SRC-001 milestones checked as marked in TODO.
- Strict Wikimedia Structured Contents/regular-source joins, public-dump staging/import tools, revision-pinned online title/page/search and bounded caches have substantial tests, but full authenticated source generation, renderer fidelity and complete visual dependencies remain unqualified.
- Immutable SQLite article catalog, frame hashes, redirect/title searches, draft pack/topic resolver, shared media storage and provenance preflight, derived FTS index and offline-reader constructors exist, but the complete installed snapshot and media activation path is not production-integrated.
- Desktop Tauri/React previews, edition-isolated tabs/navigation, bookmarks/history, light/dark iframe previews, offline-mode toggles, secure UI boundaries and privacy-deletion race protection have scoped tests. Real mandatory offline images and full article reading are not wired end-to-end.
- Android Compose/WebView shell and Rust ABI compilation exist, but callable real-device-qualified Kotlin UniFFI access, installed media, offline WebView, model manager and acceptance are still open.
- AI-001 provider-independent interface is checked; source-cited chat, local/LAN/cloud provider completion, secret storage, grounded-answer/selection UX and optional Android inference remain unfinished.

## All remaining top-level task IDs (complete inventory)

**69 open top-level IDs total: 63 in v0.1 scope plus six explicitly deferred future items.** An unchecked item may have useful implemented components and many scoped qualification notes.

- **Phase 2 — Wikimedia acquisition and revision-matched normalization** — SRC-002, SRC-003, SRC-004, SRC-005.
- **Phase 3 — Immutable snapshot and shared object storage** — STORE-001, STORE-002, STORE-003, STORE-004.
- **Phase 4 — Pack format, resolver, builder and mandatory media** — PACK-001, PACK-002, PACK-003, PACK-004, PACK-005, PACK-006.
- **Phase 5 — Publisher signatures, custom trust and hostile import protection** — TRUST-001, TRUST-002, TRUST-003.
- **Phase 6 — RAG and evidence-based answer contracts** — RAG-001, RAG-002, RAG-003, RAG-004.
- **Phase 7 — Desktop Wikipedia-like reader** — DUI-001, DUI-002, DUI-003.
- **Phase 8 — AI provider adapters, streaming and model profiles** — AI-002, AI-003, AI-004, AI-005.
- **Phase 9 — M3 Expressive action hub and desktop chat** — UX-001, UX-002, UX-003, UX-004.
- **Phase 10 — Desktop Pack Builder screens and CLI parity** — PUI-001, PUI-002, PUI-003, PUI-004, PUI-005.
- **Phase 11 — Persistent state and multi-page Settings** — STATE-001, STATE-002, STATE-003.
- **Phase 12 — Android Rust/UniFFI and Compose/WebView reader** — MOB-001, MOB-002, MOB-003, MOB-004.
- **Phase 13 — Android official/custom pack management and offline media** — MOBPACK-001, MOBPACK-002, MOBPACK-003.
- **Phase 14 — Optional local Android LLM, quality gate** — LOCAL-001, LOCAL-002, LOCAL-003.
- **Phase 15 — Official pack publication and update distribution** — UPD-001, UPD-002, UPD-003, UPD-004.
- **Phase 16 — Security, licensing and privacy** — SEC-001, SEC-002, SEC-003.
- **Phase 17 — Qualifying UX, reliability and performance** — QA-001, QA-002, QA-003.
- **Phase 18 — Packaging and end-to-end v0.1 release** — REL-001, REL-002, REL-003.
- **Deferred — explicitly not v0.1 blockers** — FUT-001, FUT-002, FUT-003, FUT-004, FUT-005, FUT-006.

## Recommended next work, in dependency order

1. **Finish STORE-004 read-only FTS5 internal integrity trust boundary.** Investigate a bounded SQLite-consistent backup into a private writable temporary database followed by FTS5 integrity-check, without changing published bytes. Account for WAL consistency, symlinks, concurrent replacement, resource quotas, temp cleanup, read-only contract and corruption regression; consider documented activation-time guarantees if rechecking every open is too costly.
2. **First complete vertical offline path.** Connect verified immutable catalog / OfflineReader and mandatory local media to production desktop Tauri article and search commands; preserve online-current versus installed versus temporary labels. No HTTP for offline requests. Follow with actual Android UniFFI/Kotlin and safe WebView local-media reader integration.
3. **Source completeness and media.** Qualify one official completed Wikimedia publication with revision-matched structured content, rendered HTML, namespaces, redirects, category membership, images/diagrams/maps/math/captions/references and attribution. Audio/video metadata and thumbnails local; payloads online-on-demand only.
4. **Pack build, trust and update.** Deterministic category/topic packs; portable versioned .wpack containing all required still visuals; signed publisher catalog versus explicitly custom/integrity-only labels; safe import, deduped media, durable jobs, rollback and desktop/Android install.
5. **Visible product and AI.** Desktop/Android article fidelity, persisted reader scroll/back/tab state, selection/citation navigation, accessible FAB/chat/Settings, bounded grounded RAG, provider consent and secure secret storage, optional small Android local LLM only after measurement.
6. **Qualification/release.** Real device in airplane mode with complete representative science/media pack, accessible UI and citations, no covert network use, clean desktop installation, update failure recovery, license/security/benchmark tests and signed release artifacts. Do not close REL IDs without exact-head evidence.

**Priority tradeoff:** After bounded STORE-004 closure, prioritize the missing source-to-pack-to-production-offline-reader end-to-end functionality over an endless series of isolated validation micro-increments.

## Blockers and owner actions

The canonical owner-action blocker register has no active requirement recorded at this snapshot. Independent software development should continue. Publisher signing-key/hosting decisions, authenticated large real-source data, owner-owned network credentials or physical Android acceptance may need input when the relevant gate is reached. Never invent physical-device results or confer publisher authenticity from a locally supplied checksum alone.

## Next Ralph Bridge session

1. Use **Ralph Bridge exclusively** for ALL GitHub reads/writes/CI; never the default GitHub tool. Only master, compare-and-swap exact expected head SHA, no task-by-task branches or PRs.
2. Re-fetch fresh master SHA, full canonical TODO, SPEC, current handoff, relevant code and exact-head CI. Do not assume the snapshot in this handoff is still the current head.
3. The TODO is very large. Read ALL contiguous line windows and verify the total line count and byte integrity before any rewrite; NEVER replace it with a truncated first window. Account for concurrent modifications.
4. Implement a coherent task slice, test formatting/Clippy/workspace and relevant desktop/Android integration, commit to master with exact-head guard, inspect the new SHA and terminal CI, fix failures, then record precise evidence and remaining acceptance gaps.
5. Keep still-unqualified parent tasks unchecked until all mandatory automated/manual/device criteria pass. Do not equate code existence with complete full-product acceptance.

## Quick reference

- [Canonical TODO](WIKIPEDIA_AI_READER_TODO.md) · [SPEC](WIKIPEDIA_AI_READER_SPEC.md) · [Development instructions](DEVELOPMENT.md)
- [Latest FTS integrity qualification](STORE_004_NATIVE_FTS5_INTEGRITY_CHECK_2026-10-28.md)

**Handoff conclusion:** 14.6% of checkbox entries and 10.4% of top-level IDs are fully checked. Most release-critical integrations and acceptance remain open. Source baseline 3d06631f1c1c3bc1edb54608dd51f8795fa5ebc1 is CI-green; this handoff file lives in its own subsequent master commit.
