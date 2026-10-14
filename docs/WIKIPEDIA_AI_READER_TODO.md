# Foundation Wikipedia — Canonical Implementation TODO

**Status:** Implementation underway; only qualified task IDs are marked completed  
**Created:** 2026-10-09  
**Governing requirements:** [WIKIPEDIA_AI_READER_SPEC.md](WIKIPEDIA_AI_READER_SPEC.md)  
**Repository:** https://github.com/ekkus93/foundation-wikipedia  
**Working branch:** `master` only, as requested; do not create one branch or PR per task.

## Rules for Ralph Loop and other implementation sessions

This is the **sole authoritative implementation checklist**. The SPEC is the source of product requirements. Before each development run, read the latest SPEC and TODO from current `master`, examine repository/CI status, and select the earliest dependency-satisfied unchecked work (independent items may proceed in parallel). After a coherent completed slice, commit to `master`, verify exact-head state and CI, reconcile this TODO and continue.

- Leave an item unchecked until its code, automated tests, relevant real-device/manual checks and documentation are all completed and evidenced. Never mark tasks done based on intention or a stale passing CI run.
- Record for each checked top-level ID: exact SHA, test command/result, CI run URL/ID (or “not configured”), device/OS when applicable, and artifact or benchmark links. Failures are evidence too, but do not qualify completion.
- Dependencies are hints for safe sequencing, not an excuse to serialize unrelated work unnecessarily.
- Do not create per-task branches or PRs. Observe repository permissions/branch protection rather than bypass them. Use Ralph Bridge for GitHub/CI, not the default GitHub tool, in the user's Ralph workflow.
- Ordinary test failures, transient tool failures, running CI, merge mistakes and design bugs are not user blockers. Reserve the Blockers register for missing authorization, external credentials, unavailable necessary hardware, or choices genuinely requiring the owner.
- If a technical evaluation fails, record the result and choose the next defensible implementation; do not mark required behaviors “deferred” without explicitly revising the SPEC.
- **No implementation task is marked completed by the creation of these planning documents.**

## Phase 0 — Repository/bootstrap, build and quality gates

- [x] **BOOT-001** Establish the Rust workspace. **Dependencies:** none.  
  - [x] Create `wiki-model`, `wiki-core`, `wiki-source`, `wiki-store`, `wiki-search`, `wiki-ai`, `wiki-pack-format`, `wiki-pack-builder`, `wiki-ffi` crates and `wiki-pack` CLI skeleton.
  - [x] Enforce that shared crates do not import UI/Tauri/Android dependencies.
  - [x] Pin toolchains/lockfiles; add `cargo fmt`, `clippy`, unit-test scripts.
  - **Accept:** clean checkout compiles and tests minimal workspace without external infrastructure.
- [x] **BOOT-002** Scaffold desktop and Android shells. **Depends:** BOOT-001.  
  - [x] Tauri 2/React/TypeScript/Vite desktop and Kotlin/Jetpack Compose/Gradle Android shells.
  - [x] Android Rust build/ABI plan and UniFFI build integration stub.
  - [x] Document development prerequisites and reproducible clean-build commands.
  - **Accept:** desktop launches and Android debug APK installs from clean checkout.
- [x] **BOOT-003** Establish exact-head CI and documentation discipline. **Depends:** BOOT-002.  
  - [x] Rust fmt/clippy/test, frontend lint/typecheck/test, Android tests/lint, dependency/license audit.
  - [x] Fixture data provenance and test artifact retention.
  - [x] Add ADR and requirement-to-TODO traceability checklist.
  - **Accept:** CI detects intentionally introduced failures and reports exact commit SHA.

## Phase 1 — Canonical model and provenance

- [x] **MOD-001** Implement versioned article/section/block/media/reference/link/redirect models. **Depends:** BOOT-001.  
  - [x] Key articles on project + page ID; preserve revision ID, timestamp, title/aliases and optional Wikidata ID.
  - [x] Support paragraphs, nested sections, tables, lists, quote, math, media, infobox, footnotes, HTML fallback and disambiguation.
  - [x] Add serde/Unicode roundtrip fixtures and schema validation.
  - **Accept:** complete varied fixtures can serialize, deserialize and render without dropping required semantics.
- [x] **MOD-002** Build deterministic revision-scoped IDs and evidence maps. **Depends:** MOD-001.  
  - [x] Assign block, section, footnote and reference IDs from an exact revision.
  - [x] Separate hard citation IDs from soft bookmark/highlight relocation anchors.
  - [x] Tests for text reorder, deletion and revision changes.
  - **Accept:** stale AI citation never points to unrelated text after content update.
- [x] **MOD-003** Select canonical record encoding and compatibility policy. **Depends:** MOD-001.  
  - [x] Compare CBOR/MessagePack + record-level zstd/shard sizes against measured fixture performance.
  - [x] Record Android memory, random read, compression and migration results in ADR.
  - [x] Implement major/minor version guards and explicit reject/migrate behavior.
  - **Accept:** proven versioned codec and safe handling of unsupported revisions.

## Phase 2 — Wikimedia acquisition and revision-matched normalization

- [x] **SRC-001** Implement verified official-source acquisition. **Depends:** MOD-001.  
  - [x] Discover *completed* Wikimedia source generations, not guessed filenames; capture authority, release ID, project, date, URLs, hashes.
  - [x] Download resume, timeout/rate-limit handling, configured mirror/local file transport and partial-data isolation.
  - [x] Verify source checksums; fail closed on mismatch or incomplete source.
  - **Accept:** interrupted source fetch resumes; corrupted mirror bytes rejected with no active-snapshot damage.
- [ ] **SRC-002** Implement Structured Contents adapter and necessary **other** official data joins. **Depends:** SRC-001.  
  - [x] Parse beta structured sections/references/infoboxes/tables.
  - [x] Obtain matching rendered body/HTML, categories, redirects, namespace data from official source(s); don't assume Structured Contents has those fields.
  - [x] Enforce project/page/revision equality across imported components; detect duplicates and deleted pages.
  - **Accept:** article HTML, retrieved blocks and category membership have proven consistent provenance.
  - **Progress (2026-10-14):** Scoped verified import and exact-head CI evidence: [SRC-002 qualification](SRC_002_SCOPED_IMPORT_QUALIFICATION_2026-10-14.md). External source authenticity/completeness and end-to-end acceptance remain outstanding.
- [ ] **SRC-003** Qualify an official public-dump fallback. **Depends:** SRC-001.  
  - [ ] Inspect current-content dump/checksum contract and build importer.
  - [ ] Evaluate Parsoid/MediaWiki-compatible rendering for templates, Lua, math and references.
  - [ ] Record source fidelity and hosting/availability tradeoffs; implement defensible fallback.
  - **Accept:** representative public-dump articles render/read and can be packed without mandatory paid API access.
  - **Progress (2026-10-14):** [Renderer evaluation](SRC_003_RENDERER_EVALUATION_2026-10-14.md) and source-census identity validation are implemented/evaluated; renderer fidelity, independent completeness and mandatory media remain outstanding.
  - **Progress (2026-10-14):** [Pinned-ingest census export integration](SRC_003_CENSUS_PIPELINE_2026-10-14.md) passed exact-head Rust and Platform CI. Authentication, independent expected census, actual XML/bzip2 decoding and renderer fidelity remain outstanding.
  - **Progress (2026-10-14):** [Private observed-census staging](SRC_003_CENSUS_PIPELINE_2026-10-14.md) now atomically publishes only a completed pinned-event census without overwriting prior results. Exact-head Rust and Platform CI passed at `43f17e9fe80edb07ea74ded8792db7c3182b60d6`; this still requires independently verified upstream compressed members and an independent expected census.
  - **Progress (2026-10-14):** A standalone Python diagnostic decoder now verifies locally staged bzip2 members against the source manifest, parses well-formed XML, checks outer page/revision IDs and shard ordering, and publishes a namespace-filtered observed census only after all members pass. [Qualification and limits](SRC_003_CENSUS_PIPELINE_2026-10-14.md). This is not the portable Rust importer or independent upstream completeness/renderer acceptance.
  - **Qualification (2026-10-14):** Diagnostic bzip2/XML decoder and same-descriptor digest revalidation passed exact-head Rust and Platform CI at `2557eb055f212a7511d56657e50e2035081ad278` ([Rust](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38633971602), [Platform](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38633971483)). `SRC-003` remains unchecked.
  - **Progress (2026-10-14):** The Python diagnostic XML path can now optionally compare its complete staged observed census against a separately supplied SHA-256-pinned expected census **before publication**. It rejects missing/extra/malformed/unsorted IDs, digest mismatch and symlinked expected input, with bounded-memory streaming. Qualified on `007b16e023ce30e31851216ce147e2dc6c1c4dbc` ([Rust](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38649085525), [Platform](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38649085131)). The caller must still authenticate the expected census independently and establish matching cutoff/namespace semantics; this does not qualify `SRC-003`.
- [ ] **SRC-004** Implement online article/search and revision-aware caches. **Depends:** SRC-002 or SRC-003, MOD-002.  
  - [ ] Fetch search results and article by title/page ID, normalize redirects, capture exact revision.
  - [ ] Distinguish online current version, installed offline version and temporary cached version.
  - [ ] Respect offline state and API failure paths.
  - **Accept:** online article opens; offline missing article shows sensible state, not broken WebView.
- [ ] **SRC-005** Establish authoritative-like fixture suite and source regression tests. **Depends:** SRC-002, SRC-003.  
  - [ ] Duplicate/deleted/redirect/category-cycle and beta-schema-change cases.
  - [ ] Maths, infoboxes, figures, table/reference and non-English fixtures.
  - **Accept:** deterministic import output with actionable drift/failure diagnostics.

## Phase 3 — Immutable snapshot and shared object storage

- [ ] **STORE-001** Implement SQLite catalog and independently addressable compressed article records. **Depends:** MOD-003.  
  - [ ] Index page ID/title/revision/redirect/Wikidata; bounded single-article decompression.
  - [ ] Verify bytes/hashes and handle missing/corrupt shard read.
  - **Accept:** random lookup doesn't decompress an entire full-Wikipedia archive.
  - **Progress (2026-10-14):** Shard path hardening and regression tests landed at `5af57b9e8304d0515868d295a3ccabb4cb7ce552`; Rust, Platform and Android ABI checks passed. Emulator codec probe remains pending; full acceptance is not claimed.
- [ ] **STORE-002** Implement content-addressed media storage and owners. **Depends:** STORE-001.  
  - [ ] Hash/store media variants plus author/license metadata; dedupe across pack boundaries.
  - [ ] Reference accounting/GC, separate cached and user-pinned content ownership.
  - **Accept:** delete Physics pack while shared Mathematics image remains readable.
- [ ] **STORE-003** Implement staged snapshot activation, rollback and user-state isolation. **Depends:** STORE-001.  
  - [ ] Atomic content activation with old version retained; run crash/disk-full fault injection.
  - [ ] Keep mutable user SQLite separate; search/embeddings derived rebuildable.
  - **Accept:** forced failure before activation leaves prior content and bookmarks intact.
- [ ] **STORE-004** Provide offline full-text/lexical index and rebuild path. **Depends:** STORE-001, MOD-002.  
  - [ ] Benchmark Tantivy or equivalent; implement current-article retrieval block index and pack search.
  - [ ] Rebuild indexes after corruption without re-downloading media/content.
  - **Accept:** search across installed topic pack works with network disabled.

## Phase 4 — Pack format, resolver, builder and mandatory media

- [ ] **PACK-001** Specify and implement portable versioned `.wpack` container. **Depends:** MOD-003, STORE-002.  
  - [ ] Canonical manifest, object digests, source snapshot/definition hashes, origin, licensing, indexes and counts.
  - [ ] Evaluate safe streamed export/install on Android; document chosen archive/serialization ADR.
  - **Accept:** roundtrip pack through independent desktop/Android-compatible importer.
- [ ] **PACK-002** Implement deterministic topic/category resolver. **Depends:** SRC-002, PACK-001.  
  - [ ] Curated roots, explicit include/exclude, page IDs, redirects, disambiguation and stable ordering.
  - [ ] Detect graph cycles, admin categories and explosive recursion; bounded related-article option.
  - [ ] Show resolved counts and unusual-inclusion warnings before media download.
  - **Accept:** same source and definition yield identical page-ID manifest every run.
- [ ] **PACK-003** Guarantee all required offline visuals. **Depends:** PACK-001, SRC-002.  
  - [ ] Discover every article-rendered image, map, diagram, plot, caption, equation/MathML/SVG, required CSS/font.
  - [ ] Sanitize SVG/MathML; optimize raster while keeping diagram labels legible and zoomable.
  - [ ] Deduplicate media and preserve individual source/license/creator details.
  - [ ] Reject incomplete mandatory media rather than labeling the pack offline-ready.
  - **Accept:** representative mathematics/physics articles render *every required visual* in airplane mode.
- [ ] **PACK-004** Build pack pipeline and CLI. **Depends:** PACK-002, PACK-003, STORE-004.  
  - [ ] Source → selection → revisions → assets → transforms → indexes/RAG chunks → verify → artifact.
  - [ ] Implement `wiki-pack import/build/inspect/verify/export` commands backed by Rust library.
  - [ ] Report measured article/media/index/download/installed bytes, overlap savings and free space.
  - **Accept:** end-to-end exported pack installs/searches/renders offline and validates hashes.
- [ ] **PACK-005** Add curated initial topic definitions. **Depends:** PACK-002.  
  - [ ] Mathematics, Physics and Computer Science; test category coverage and fan-out.
  - [ ] Preserve editable definitions and reproducible resolved page-ID results.
  - **Accept:** independent fixture snapshots can rebuild the same defined topics.
- [ ] **PACK-006** Durable long-running builder jobs. **Depends:** PACK-004.  
  - [ ] Staged progress, cancel, retry/restart/resume policy and persisted diagnostics.
  - [ ] Never display incomplete artifact as installable or signed.
  - **Accept:** simulated process interruption leaves a recoverable job and no corrupt installed pack.

## Phase 5 — Publisher signatures, custom trust and hostile import protection

- [ ] **TRUST-001** Implement official signing and trust-root verification. **Depends:** PACK-001.  
  - [ ] Canonical signature scope, pinned publisher key, key rotation/revocation procedure and test vectors.
  - [ ] Verify origin + integrity for “Verified publisher”, not just a matching checksum.
  - **Accept:** tampered/forged official pack cannot install or display as verified.
- [ ] **TRUST-002** Show distinct trust statuses across all pack surfaces. **Depends:** TRUST-001.  
  - [ ] `Verified publisher`, `Custom — integrity verified`, `Not yet checked`, `Verification failed` with labels and accessible descriptions.
  - [ ] On all library cards, pack details and install/import confirmations.
  - [ ] Block invalid/unverified installation; allow validated custom unsigned packs after clear confirmation.
  - **Accept:** an imported custom pack never masquerades as an official pack.
- [ ] **TRUST-003** Harden pack inputs. **Depends:** PACK-004.  
  - [ ] Protect against traversal, zip bombs, oversized objects, MIME spoofing, malformed manifests and unsafe HTML/SVG.
  - [ ] Validate origin, content handlers and file-size quotas even for signed content.
  - **Accept:** malicious fixture suite fails closed with no filesystem escape or process crash.

## Phase 6 — RAG and evidence-based answer contracts

- [ ] **RAG-001** Current-article lexical/structural retrieval. **Depends:** MOD-002, STORE-004.  
  - [ ] Rank headings/sections/selected blocks and lexical candidates; enforce token budget.
  - [ ] Handle unanswerable questions and revision mismatch conservatively.
  - **Accept:** labeled Q&A retrieves relevant source blocks without a vector DB.
- [ ] **RAG-002** Citation validation pipeline. **Depends:** RAG-001.  
  - [ ] Assign evidence IDs only to retrieved blocks; verify model-reported references.
  - [ ] Store exact revision/snapshot and jump targets; mark stale citations on updates.
  - [ ] Test hallucinated IDs, unsupported answers and prompt injection inside article content.
  - **Accept:** every visible citation points to actual supplied article evidence.
- [ ] **RAG-003** Build evaluation corpus and retrieval regression tests. **Depends:** RAG-002.  
  - [ ] Factual, explanation, multi-section, selection, ambiguity and no-answer questions.
  - [ ] Measure passage recall, answer support and citation precision; log reproducible test data.
  - **Accept:** regressions visible with score report and exact tested commit.
- [ ] **RAG-004** Optional semantic/hybrid retrieval evaluation. **Depends:** RAG-003.  
  - [ ] Benchmark lexical vs embeddings vs hybrid/rerank for benefit, storage, index time and mobile RAM.
  - [ ] Keep semantic indexes derived and separately rebuildable; ship only if justified.
  - **Accept:** evidence-backed ADR records include/defer without requiring full-corpus vector DB.

## Phase 7 — Desktop Wikipedia-like reader

- [ ] **DUI-001** Create functional Tauri/React article reader/search. **Depends:** BOOT-002, SRC-004.  
  - [ ] Familiar Wikipedia desktop layout, headings, infoboxes, references, tables, image and math rendering.
  - [ ] Internal link interception, external-browser policy, redirects, desktop resizing and themes.
  - **Accept:** varied live and fixture articles browse in desktop app.
- [ ] **DUI-002** Exact citation highlight and safe selection bridge. **Depends:** DUI-001, MOD-002.  
  - [ ] Render stable block anchors and scroll/highlight citations.
  - [ ] Report selection with page/revision/block context; safe local media protocol and controlled origins.
  - [ ] Test offline image and equation loads without network.
  - **Accept:** AI citation highlights exact source block; no privileged bridge leakage.
- [ ] **DUI-003** Desktop tabs/back/history/reading-position behavior. **Depends:** DUI-001.  
  - [ ] Preserve article state through tab switches and links; handle missing offline target.
  - **Accept:** return to earlier article at original position.

## Phase 8 — AI provider adapters, streaming and model profiles

- [x] **AI-001** Define provider-independent Rust LLM interface and stream events. **Depends:** BOOT-001, RAG-001.  
  - [x] Providers enumerate models where possible, test connection, stream/cancel, expose capabilities and typed failures.
  - [x] Fake provider tests for success, timeout, bad response and cancellation.
  - **Accept:** RAG orchestration contains no provider-specific UI code.
- [ ] **AI-002** Localhost/LAN provider support. **Depends:** AI-001.  
  - [ ] OpenAI-compatible endpoints and Ollama/llama-server adapters; user-selected endpoint/model.
  - [ ] Correct LAN versus on-device locality indicator.
