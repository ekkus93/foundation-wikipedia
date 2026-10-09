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
- [ ] **BOOT-002** Scaffold desktop and Android shells. **Depends:** BOOT-001.  
  - [x] Tauri 2/React/TypeScript/Vite desktop and Kotlin/Jetpack Compose/Gradle Android shells.
  - [ ] Android Rust build/ABI plan and UniFFI build integration stub.
  - [x] Document development prerequisites and reproducible clean-build commands.
  - **Accept:** desktop launches and Android debug APK installs from clean checkout.
- [ ] **BOOT-003** Establish exact-head CI and documentation discipline. **Depends:** BOOT-002.  
  - [ ] Rust fmt/clippy/test, frontend lint/typecheck/test, Android tests/lint, dependency/license audit.
  - [ ] Fixture data provenance and test artifact retention.
  - [x] Add ADR and requirement-to-TODO traceability checklist.
  - **Accept:** CI detects intentionally introduced failures and reports exact commit SHA.

## Phase 1 — Canonical model and provenance

- [ ] **MOD-001** Implement versioned article/section/block/media/reference/link/redirect models. **Depends:** BOOT-001.  
  - [ ] Key articles on project + page ID; preserve revision ID, timestamp, title/aliases and optional Wikidata ID.
  - [ ] Support paragraphs, nested sections, tables, lists, quote, math, media, infobox, HTML fallback and disambiguation.
  - [ ] Add serde/Unicode roundtrip fixtures and schema validation.
  - **Accept:** complete varied fixtures can serialize, deserialize and render without dropping required semantics.
- [ ] **MOD-002** Build deterministic revision-scoped IDs and evidence maps. **Depends:** MOD-001.  
  - [ ] Assign block, section, footnote and reference IDs from an exact revision.
  - [ ] Separate hard citation IDs from soft bookmark/highlight relocation anchors.
  - [ ] Tests for text reorder, deletion and revision changes.
  - **Accept:** stale AI citation never points to unrelated text after content update.
- [ ] **MOD-003** Select canonical record encoding and compatibility policy. **Depends:** MOD-001.  
  - [ ] Compare CBOR/MessagePack + record-level zstd/shard sizes against measured fixture performance.
  - [ ] Record Android memory, random read, compression and migration results in ADR.
  - [ ] Implement major/minor version guards and explicit reject/migrate behavior.
  - **Accept:** proven versioned codec and safe handling of unsupported revisions.

## Phase 2 — Wikimedia acquisition and revision-matched normalization

- [ ] **SRC-001** Implement verified official-source acquisition. **Depends:** MOD-001.  
  - [ ] Discover *completed* Wikimedia source generations, not guessed filenames; capture authority, release ID, project, date, URLs, hashes.
  - [ ] Download resume, timeout/rate-limit handling, configured mirror/local file transport and partial-data isolation.
  - [ ] Verify source checksums; fail closed on mismatch or incomplete source.
  - **Accept:** interrupted source fetch resumes; corrupted mirror bytes rejected with no active-snapshot damage.
- [ ] **SRC-002** Implement Structured Contents adapter and necessary **other** official data joins. **Depends:** SRC-001.  
  - [ ] Parse beta structured sections/references/infoboxes/tables.
  - [ ] Obtain matching rendered body/HTML, categories, redirects, namespace data from official source(s); don't assume Structured Contents has those fields.
  - [ ] Enforce project/page/revision equality across imported components; detect duplicates and deleted pages.
  - **Accept:** article HTML, retrieved blocks and category membership have proven consistent provenance.
- [ ] **SRC-003** Qualify an official public-dump fallback. **Depends:** SRC-001.  
  - [ ] Inspect current-content dump/checksum contract and build importer.
  - [ ] Evaluate Parsoid/MediaWiki-compatible rendering for templates, Lua, math and references.
  - [ ] Record source fidelity and hosting/availability tradeoffs; implement defensible fallback.
  - **Accept:** representative public-dump articles render/read and can be packed without mandatory paid API access.
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
  - **Accept:** local server supplies grounded answer without cloud calls.
- [ ] **AI-003** Remote provider support and secret storage. **Depends:** AI-001.  
  - [ ] Hosted OpenAI, Anthropic and Gemini adapters as appropriately distinct APIs.
  - [ ] Keys in OS secret storage; masked UI, no accidental logs or pack exports.
  - **Accept:** explicit remote request succeeds; credentials do not appear in diagnostics.
- [ ] **AI-004** Profiles, consent and fallback behavior. **Depends:** AI-002, AI-003.  
  - [ ] Saved profiles; enable/disable, model selection and connection testing.
  - [ ] Clear on-device/LAN/cloud label and disclosure of passages sent; no silent cloud fallback.
  - **Accept:** a failed local model never causes remote traffic without user consent.
- [ ] **AI-005** Chat/RAG orchestration, persistence hook and citation stream. **Depends:** AI-001, RAG-002.  
  - [ ] Chat scoped to article and exact revision; selection context; bounded history.
  - [ ] Display unsupported-answer state; cancellation and valid citation links.
  - **Accept:** streaming Q&A with citations works using both fake and real provider.

## Phase 9 — M3 Expressive action hub and desktop chat

- [ ] **UX-001** Bottom-right expressive FAB/speed dial. **Depends:** DUI-001.  
  - [ ] Expand/collapse, labels; stable nearest-first actions Chat, Bookmark, Offline, Settings.
  - [ ] Keyboard, touch, reduced-motion, screen-reader labels, outside click/Escape and focus restoration.
  - **Accept:** overlay never changes Wikipedia layout or blocks standard navigation.
- [ ] **UX-002** Movable desktop chat surface. **Depends:** UX-001, AI-005.  
  - [ ] Draggable/resizable/minimizable floating window constrained to viewport; streaming and provenance.
  - [ ] Citation click and selected-text actions; keep article-scoped conversation when closed.
  - **Accept:** chat and citation-scroll continue to work after returning from Settings.
- [ ] **UX-003** Bookmark FAB action and management. **Depends:** UX-001, STATE-001.  
  - [ ] One-click toggle, accessible confirmation, latest available revision reopening and safe anchors.
  - **Accept:** bookmarks survive restart and snapshot updates.
- [ ] **UX-004** Offline FAB action. **Depends:** UX-001, PACK-004, STATE-001.  
  - [ ] Explicit/pack-owned/cached/online-only states; download full article + images, progress and real size.
  - [ ] Prevent removing pack-owned shared objects.
  - **Accept:** individually saved article works offline even after clearing temporary cache.

## Phase 10 — Desktop Pack Builder screens and CLI parity

- [ ] **PUI-001** Wikipedia Packs library and Source Data screens. **Depends:** DUI-001, PACK-006, TRUST-002.  
  - [ ] Installed/Official/My/Create/Source destinations; source import/update status, trust labels, storage display.
  - **Accept:** user can inspect source and existing packs without terminal.
- [ ] **PUI-002** Wizard Basics/Content/Advanced. **Depends:** PUI-001, PACK-002.  
  - [ ] Named persistent pack definition; expressive curated topic tiles, categories, page additions/exclusions, recursion warnings.
  - **Accept:** save/edit/reopen definition without losing rules.
- [ ] **PUI-003** Selection review, required media and search settings. **Depends:** PUI-002, PACK-003.  
  - [ ] Preview resolved articles and potential topic leaks; mandatory visual policy not disableable.
  - [ ] Audio/video described as online on demand; optional semantic index not forced.
  - **Accept:** user sees selection/media warnings before expensive build begins.
- [ ] **PUI-004** Build Estimate, Progress, Completion. **Depends:** PUI-003, PACK-006.  
  - [ ] Measured data/images/index bytes, dedupe savings, free disk; expressive but accessible progress UI.
  - [ ] Durable jobs, cancel/restart/diagnostics; install locally/export `.wpack`/inspect/save definition.
  - **Accept:** completed artifact validates and remains available after navigating away during build.
- [ ] **PUI-005** CLI/GUI engine parity. **Depends:** PUI-004.  
  - [ ] Ensure same library, manifest semantics and error states in GUI and CLI.
  - **Accept:** equivalent source/definition yields equivalent verified objects/manifests.

## Phase 11 — Persistent state and multi-page Settings

- [ ] **STATE-001** Mutable SQLite state schema and migrations. **Depends:** STORE-003.  
  - [ ] Bookmarks, reading history/position, chat by article/revision, settings, pack ownership and job inventory.
  - [ ] User-data deletion and optional history saving; separate canonical source from mutable state.
  - **Accept:** state survives restart without corrupting offline packs.
- [ ] **STATE-002** Desktop Settings categories and return-state behavior. **Depends:** STATE-001, UX-001.  
  - [ ] General, Appearance & Reading, AI Assistant, Wikipedia & Offline Data, Storage, History & Saved Content, Privacy & Network, Advanced, About.
  - [ ] Desktop category rail, settings persistence, restore page/revision/tab/scroll/sections/chat position when leaving.
  - **Accept:** Settings→Back returns to same article and reading position.
- [ ] **STATE-003** Capability-aware settings and secure preferences. **Depends:** STATE-002, AI-004.  
  - [ ] Secret store, deletion confirmation, app-specific desktop versus Android options, sensible defaults.
  - [ ] Validate network/cellular, A/V playback and grounded-answer policies.
  - **Accept:** no advanced search knobs in normal flow; no unsupported controls on wrong platform.

## Phase 12 — Android Rust/UniFFI and Compose/WebView reader

- [ ] **MOB-001** UniFFI Kotlin bindings and native ABI builds. **Depends:** BOOT-002, MOD-001.  
  - [ ] arm64 native library, coroutine-safe calls/events, typed errors, cancellation and no main-thread work.
  - [ ] Test on a physical Android device and emulator where relevant.
  - **Accept:** Android search/article retrieval uses shared Rust code.
- [ ] **MOB-002** Android Wikipedia-like WebView reader. **Depends:** MOB-001, SRC-004.  
  - [ ] Compose navigation/search, article body WebView, safe local-media origin, link interception and Back stack.
  - [ ] Offline images/SVG/math/captions, full-screen image zoom, rotation and lifecycle state.
  - **Accept:** visual article works in airplane mode and matches mobile Wikipedia expectations.
- [ ] **MOB-003** Android M3 Expressive FAB and Compose chat bottom sheet. **Depends:** MOB-002, AI-005.  
  - [ ] Stable global actions, sheet peek/half/full, streamed chat, selection and citations to WebView.
  - [ ] Touch/TalkBack/dynamic color/reduced motion.
  - **Accept:** select text→ask local/remote model→tap citation→correct paragraph highlight.
- [ ] **MOB-004** Android Settings, Bookmarks, History and reader return. **Depends:** MOB-002, STATE-003.  
  - [ ] Native subpage navigation, Android Back to exact article state, correct platform-specific settings.
  - [ ] Restore session after process recreation.
  - **Accept:** Settings exit returns to same wiki page/scroll and open-chat state.

## Phase 13 — Android official/custom pack management and offline media

- [ ] **MOBPACK-001** Official catalog, verified downloads and install manager. **Depends:** PACK-004, TRUST-002, MOB-001.  
  - [ ] Signed publisher catalog discovery, resumable download, real disk estimates, validation before activation.
  - [ ] Network/Wi-Fi restrictions and trust status visible.
  - **Accept:** verified official pack remains readable offline after restart.
- [ ] **MOBPACK-002** Custom `.wpack` import via Android document picker. **Depends:** MOBPACK-001.  
  - [ ] Display Custom — integrity verified, never publisher-verified; reject tampering.
  - [ ] Deduplicate with existing pack and remove safely.
  - **Accept:** desktop-built custom pack imports; shared media remains after another pack removed.
- [ ] **MOBPACK-003** Airplane-mode and A/V-on-demand acceptance. **Depends:** MOBPACK-002, MOB-002.  
  - [ ] No missing images, diagrams, math, tables or reference data and no hidden network requests.
  - [ ] A/V preview/metadata local; disconnected tap shows Internet requirement; connected tap streams per policy.
  - **Accept:** representative math/science packs fully readable offline, on-device search works.

## Phase 14 — Optional local Android LLM, quality gate

- [ ] **LOCAL-001** Select Android inference engine/model through benchmarks. **Depends:** MOB-001, RAG-003.  
  - [ ] Evaluate ~0.8B Q4 model (Qwen3.5-0.8B candidate) against runtime variants and licenses.
  - [ ] Measure memory/OOM, startup, tokens/sec, context 4K–8K, thermal, battery, ABI and cancellation on actual devices.
  - **Accept:** written ADR and documented qualified-device thresholds.
- [ ] **LOCAL-002** Verified downloadable model manager. **Depends:** LOCAL-001, STATE-001.  
  - [ ] Opt-in download outside base APK, manifest + hash/license checks, progress, disk cleanup/removal.
  - [ ] Mark as on-device, separate from LAN/local-network profiles.
  - **Accept:** install/verify/model selection survives restart and needs no network for inference.
- [ ] **LOCAL-003** Grounded offline AI and error recovery. **Depends:** LOCAL-002, AI-005, MOBPACK-003.  
  - [ ] Evaluate article-grounded answers/citations using same RAG corpus and compact prompt.
  - [ ] Handle model missing/OOM/throttling/timeout/cancel; no hidden remote fallback.
  - **Accept:** qualified device answers offline article question with genuine citations in airplane mode.

## Phase 15 — Official pack publication and update distribution

- [ ] **UPD-001** Choose documented publisher catalog hosting/keys and initial release pipeline. **Depends:** TRUST-001, PACK-004.  
  - [ ] Evaluate CDN/hosting/mirror, signed catalog, stable URLs, transfer size, storage and credentials.
  - [ ] Produce a test official pack tied to an authoritative Wikimedia snapshot.
  - **Accept:** desktop/Android clean install can discover and cryptographically verify published pack.
- [ ] **UPD-002** Whole-pack update, atomic activation and rollback. **Depends:** UPD-001, STORE-003.  
  - [ ] Detect newer pack with real size/trust; download/resume/stage/verify; preserve previous version.
  - [ ] User-configurable update check and connectivity policy; no destructive silent replacement.
  - **Accept:** forced checksum/network/disk failure leaves last working version usable.
- [ ] **UPD-003** Custom pack rebuild from updated official source. **Depends:** SRC-002, PUI-004.  
  - [ ] Reload definition, compare resolved IDs/revisions/media, rebuild new snapshot artifact.
  - [ ] Export/import on Android without labeling custom artifact as official.
  - **Accept:** one-click rebuild path retains user selection and validates new source provenance.
- [ ] **UPD-004** Separate binary and content updates. **Depends:** UPD-002, MOD-003.  
  - [ ] App update preserves installed compatible packs/state; invalid major schema rejected clearly.
  - [ ] No unnecessary whole-Wikipedia re-download on app update.
  - **Accept:** simulated app update + older pack gracefully migrates or safely declines.

## Phase 16 — Security, licensing and privacy

- [ ] **SEC-001** Threat model and secure reader/import/bridge review. **Depends:** TRUST-003, DUI-002, MOB-002.  
  - [ ] Hostile HTML/SVG, archive traversal/bombs, compromised signing key, prompt injection, WebView JS bridge, dangerous URLs.
  - [ ] Negative test fixtures and no privileged arbitrary JS/channel access.
  - **Accept:** adversarial tests reject unsafe content and preserve storage integrity.
- [ ] **SEC-002** Attribution and license correctness. **Depends:** PACK-003, DUI-001.  
  - [ ] Article revision/license/contributor links; per-image creator/source/license/derivative attribution.
  - [ ] Pack-level notices and attribution visible even offline; Wikimedia branding/non-affiliation review.
  - **Accept:** offline sample export contains attribution needed for all embedded sample media.
- [ ] **SEC-003** LLM privacy and credentials audit. **Depends:** AI-004, STATE-003.  
  - [ ] Inspect network for hidden cloud fallback/telemetry; log redaction and secure secret storage.
  - [ ] Explicit local/LAN/cloud endpoint clarity and remote-context disclosure.
  - **Accept:** selected local-only provider causes zero hosted-AI traffic.

## Phase 17 — Qualifying UX, reliability and performance

- [ ] **QA-001** Wikipedia visual fidelity + expressive UI and accessibility. **Depends:** PUI-004, MOB-004, UX-002.  
  - [ ] Test representative layouts/themes/desktop widths/Android orientations.
  - [ ] Keyboard/TalkBack, labels, focus, color contrast and reduced motion; Settings subpage behavior.
  - **Accept:** documented screenshots and accessibility verification on supported configurations.
- [ ] **QA-002** Measure performance budgets. **Depends:** MOBPACK-003, LOCAL-003.  
  - [ ] Measure cold start, HTML rendering, scroll, local search, pack build/update, RAM, disk, local inference/thermal.
  - [ ] Set measured performance thresholds and add regression checks.
  - **Accept:** reproducible metrics with supported-hardware details.
- [ ] **QA-003** Integration fixture and offline regression matrix. **Depends:** QA-001, QA-002.  
  - [ ] Long/short/science/math/media/table/redirect/disambiguation/non-English Unicode articles.
  - [ ] Corrupt snapshot, interrupted pack update, unsupported schema and missing network behavior.
  - **Accept:** exact-head test results for desktop and at least one real Android device.

## Phase 18 — Packaging and end-to-end v0.1 release

- [ ] **REL-001** Desktop distributable qualification. **Depends:** QA-003, UPD-004, SEC-002.  
  - [ ] Produce self-contained Tauri desktop package(s) for supported OS, stable persistent data paths.
  - [ ] Fresh install/import/build/export/AI/Settings/back tests.
  - **Accept:** release artifact exists, checksum published and clean-install acceptance passes.
- [ ] **REL-002** Android artifact and real-device qualification. **Depends:** QA-003, SEC-003.  
  - [ ] Produce APK/AAB for declared supported ABIs/Android versions; sign per release policy.
  - [ ] Validate real-device UniFFI, WebView, airplane-mode pack, A/V online and local model when installed.
  - **Accept:** installable APK/AAB and recorded exact-head device evidence.
- [ ] **REL-003** Cross-device complete product acceptance. **Depends:** REL-001, REL-002, UPD-003.  
  - [ ] Desktop imports authoritative Wikimedia snapshot, builds science pack with images and exports `.wpack`.
  - [ ] Android imports custom pack; shows Custom — integrity verified; airplane mode: images, diagrams, math, search, citations and downloaded local model work.
  - [ ] Official signed pack shows Verified publisher; A/V play only with connection; Settings/back preserves page/position; update failure rolls back.
  - [ ] Reconcile every SPEC MUST to implementation/test evidence and mark TODO only when true.
  - **Accept:** full exact-master-SHA release qualification report and no unacknowledged v0.1 blockers.

## Deferred — explicitly not v0.1 blockers

- [ ] **FUT-001** Native SwiftUI/WKWebView iOS application via shared Rust/UniFFI core.
- [ ] **FUT-002** Incremental per-object or binary delta transport for official pack updates.
- [ ] **FUT-003** Full Wikipedia semantic/hybrid vector search/rerank if evaluation justifies storage/cost.
- [ ] **FUT-004** Optional user-initiated offline audio/video download and management.
- [ ] **FUT-005** User accounts/cloud sync, shared annotations, complex research workspaces.
- [ ] **FUT-006** Additional mobile inference runtimes/model sizes, native article renderer where warranted.

## Qualification evidence register

Use a record like this **after** a task is genuinely qualified:

```text
Task ID:
Master SHA:
Test commands/results:
CI run URL/ID + exact SHA (or not configured):
Device/OS/ABI where applicable:
Screenshot, manifest hash, benchmark or artifacts:
Failure and recovery test:
Spec deviation/ADR:
```

### BOOT-001 — verified Rust workspace bootstrap

- Master SHA: `a466255b478fc4a8d75a35ae3afd93c4f8a39a29`
- CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/37912719275 (exact-head success; `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and head consistency check).
- Artifacts: 9 platform-neutral crates, `wiki-pack` CLI skeleton, pinned `rust-toolchain.toml`, `Cargo.lock`, `Cargo.toml` workspace, README and `docs/DEVELOPMENT.md`.
- Scope boundary: This verifies only BOOT-001. Desktop/Android scaffolds, serde codecs, functional pack building and end-user features remain unchecked.



### RAG-001 — partial article-local lexical retrieval (not complete)

- Implementation: `crates/wiki-search/src/lib.rs` ranks terms from a single validated article, boosts matching headings, bounds result count, generates revision-scoped block IDs and rejects unanswerable lexical queries without manufacturing evidence.
- Validated master SHA: `54277c69b2cc60c50eacd0e2b9e5d10fa00afb5d`.
- CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/37913138904 (Rust formatting, strict Clippy, unit tests and exact-head check all passed).
- Remaining: canonical evidence maps, production block retrieval/index and token budget, article/selection context and BM25 evaluation. `RAG-001` remains unchecked.

### SRC-001 — partial authoritative source manifest validation (not complete)

- Implementation: `crates/wiki-source/src/lib.rs` validates project IDs, generation IDs, source publication-completed state, nonempty manifests, well-formed SHA-256 hex strings and duplicate member filenames.
- Validated master SHA: `d653391dd5c6b637ce0bd75f2e7a369c73572c56`.
- CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/37914208872 (Rust checks and exact-head consistency passed).
- Remaining: authoritative release discovery, bytewise SHA-256 validation, resume/mirror transport and activation isolation. `SRC-001` remains unchecked.

### PACK-002 — partial deterministic category resolver

- Code: `crates/wiki-pack-builder/src/selection.rs` and `tests/selection.rs`: cycle prevention, stable page-ID ordering, category filtering, inclusions/exclusions, page/depth limits.
- Qualified: `af6d1c23257c7f1721c7a3a2977d221e41d660be`; https://github.com/ekkus93/foundation-wikipedia/actions/runs/37914680497 (format, Clippy, tests, exact-head check passed).
- Remaining: official category source integration, redirect/disambiguation resolution and fixed-snapshot/definition provenance. PACK-002 unchecked.

### MOD-002 — partial revision-scoped evidence mapping

- Code: `crates/wiki-core/src/evidence.rs` and `tests/evidence.rs`: validated nested sections, exact revision-bound block handles and cross-revision identity change.
- Qualified: `1eb109d6cd20a07f9ea2c0bbe0ab7200ebea1b86`; https://github.com/ekkus93/foundation-wikipedia/actions/runs/37915021866 (format, Clippy, tests, exact-head check passed).
- Remaining: footnote/reference mapping, soft anchors, HTML bridge/RAG integration. MOD-002 unchecked. An unused intermediate duplicate model draft was removed by `90fee5b0d2fc880aba7f4e1cf125dd8aef2e0c40`.

### PACK-001 — partial portable manifest validation

- Code: `crates/wiki-pack-format/src/manifest.rs` and `tests/manifest.rs`: basic origin classification, version checks, malformed digests, object path safety and duplicates.
- Qualified: `6c872b39f98d5a53d115e60949ff82f058f50873`; https://github.com/ekkus93/foundation-wikipedia/actions/runs/37916125850 (format, Clippy, tests, exact-head check passed).
- Remaining: actual .wpack framing, canonical signing inputs, source hashes/provenance, full media/index catalog, byte verification and Android import. PACK-001 unchecked. Structural validation alone never confers Verified Publisher trust.

### BOOT-002 — partial desktop and Android shell qualification (not complete)

- Platform CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/37915637440 on exact commit `d73b91c9daa4f0941ba5e12116b69c143fcef64c`; all three jobs passed: desktop React/TypeScript production bundle, Android SDK 35/JDK17 Gradle `assembleDebug` + unit tests, and native Tauri Rust 1.90 `cargo check` on Ubuntu 24.04 with WebKitGTK dependencies.
- Docs: `README.md`, `docs/DEVELOPMENT.md` and `android/README.md` describe Rust toolchains, native dependencies, Gradle and Node build instructions, limitations and Android UniFFI/ABI integration plan. Android APK artifact retention added to CI.
- Remaining: actual UniFFI Android binding/build integration, **real desktop launch**, **real Android debug APK installation and lifecycle smoke test**, and reproducible transitive npm/Gradle dependency locks. BOOT-002 **parent stays unchecked** until required acceptance is evidenced; passing compilation is not proof of launch or on-device installation.

### BOOT-002 — partial Android arm64 native ABI scaffold (not complete)

- Implemented at `2c001f60e1abcaaebc9d47e413b3c57bd2e6a2d4`: `wiki-ffi` rlib/cdylib crate types, opt-in Gradle `buildRustArm64` task and NDK API-26 cross-build script with ignored arm64-v8a output. Docs explain prerequisites and limitations.
- Exact-head CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/37918701483 (Rust workspace) and https://github.com/ekkus93/foundation-wikipedia/actions/runs/37918701458 (platform shells), both passed.
- **Not yet qualified:** the CI checks Rust workspace and Gradle shell but does not cross-compile using a real NDK; there are no UniFFI-generated bindings, exported API, device installation or desktop launch checks. BOOT-002 and its remaining integration subtask stay unchecked.

### BOOT-003 — partial ADR, fixture provenance and evidence retention (not complete)

- `docs/adr/0001-shared-rust-boundary.md`, `docs/adr/README.md` and `docs/REQUIREMENTS_TRACEABILITY.md` document architecture rationale and map SPEC sections to canonical TODO/qualification.
- `scripts/verify_fixture_provenance.py` validates committed fixture source/license/byte hash and requires official-source identity for Wikimedia fixtures. `fixtures/synthetic/gravity.txt` has a byte-verified sidecar. Rust CI now retains test logs for 14 days.
- Exact-head CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/37919628903 (Rust workspace including fixture check) and https://github.com/ekkus93/foundation-wikipedia/actions/runs/37919628827 (platform shells), both passed at `e3d4258539ecebaadf1e0230880fee9f023980ea`.
- **Remaining:** robust fixture-checker negative tests, frontend lint/tests, Android lint, dependency/license audit and intentional-failure CI qualification. BOOT-003 stays unchecked.

### 2026-10-09 — Additional partial qualification; parents remain unchecked

- **BOOT-003:** Platform CI now runs `gradle :app:lintDebug` alongside Android debug APK assembly and unit tests, retaining the Android lint HTML report and debug APK as CI artifacts. Rust CI still runs exact-head fmt/Clippy/tests and provenance checker with 14-day Rust test evidence retention. Frontend typecheck/bundle and its existing action regression test also run in CI. Full frontend lint, dependency/license auditing and deliberate-failure CI exercises remain open; do not mark BOOT-003 complete.
- **MOD-001:** Added validation and regression coverage rejecting blank article-link labels. The broader canonical model, serde/Unicode roundtrips and rendering acceptance remain open.
- **RAG-002:** Added fail-closed validation that model-claimed citation IDs belong to exact retrieved, untampered, revision-matched evidence, including a regression case where article text attempts to instruct the model to cite a fabricated ID. Retrieval, provider integration and rendered citation links remain open.
- **Exact-head qualification:** `29fbe3c3597c2f9a3585079675b0fd6aadea1f66` passed Rust workspace run 37953325502 and Platform shells run 37953325478. The interim model source truncation was repaired before this qualification; the optional Wikidata-ID validator was not retained.

### PACK-001 / TRUST-003 — partial hostile manifest validation (not complete)

- Manifest structural checks now reject control characters in object paths and blank metadata/origin names, with regression tests for NUL/newline path injection and whitespace-only publisher names.
- Qualified on exact master `fd367ad5e4012c00632d4883a041989382ca8754`: Rust workspace CI 37953712494 and Platform shells CI 37953712461 both passed.
- **Remaining:** canonical pack serialization and signing, bounded archive streaming, byte/hash verification, object media completeness, hostile archive/HTML/SVG fixtures and independent Android importer. PACK-001 and TRUST-003 stay unchecked.

### MOD-001 — further canonical provenance validation (partial)

- The canonical article validator rejects whitespace-only Wikipedia reference identifiers and missing creator metadata for media assets, with regression tests. These complement earlier checks for missing titles, revision identity, link labels, reference labels and media licensing/attribution.
- Exact master `47b88a483d1f649502fb15113aa2c783aea961d1` passed Rust workspace CI 37954809397 and Platform shells CI 37954809455.
- **Remaining:** full schema coverage, aliases, serde roundtrips and high-fidelity renderer qualification. MOD-001 remains unchecked.

### 2026-10-09 — Verified incremental work; roadmap parents remain incomplete

- **MOD-001 partial:** model rejects structurally empty paragraph/quote/HTML fallback/list/table/math/infobox content while accepting sparse valid tables/math/infoboxes. Exact master `25689803a7506dc0a8e303faefd8cfdd5ad32872`; Rust CI [37962964762](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37962964762), Platform CI [37962964753](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37962964753), both passed. Full canonical schema, serialization and rendering acceptance remain open.
- **PACK-002 partial:** redirect-chain resolution is applied to inclusion/exclusion and final page counts, with disambiguation warnings and cycle/zero-target negative tests. Exact master `8517fa798b8ac7b09615fbaa2e366e4a59097915`; Rust CI [37963293570](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963293570), Platform CI [37963293596](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963293596), both passed. Official category source integration, reproducible snapshot/definition hashes and selection-review UI remain open.
- **UX-001 partial:** React reader action hub now handles Escape, outside pointer dismissal, focus restoration and Chat-nearest-first visual order, with keyboard focus outline. Exact master `167bccccb059572a2c77dfa85fd0e6f6f3811bc1`; Rust CI [37963533859](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963533859), Platform CI [37963533977](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963533977), both passed. Real application/browser accessibility and touch acceptance remain open.
- **BOOT-003 partial:** strict TypeScript unused-local/unused-parameter lint added to platform CI. Exact master `b29739693c201455b745386a17a2c444f5fee026`; Rust CI [37963739442](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963739442), Platform CI [37963739424](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963739424), both passed.
- **BOOT-003 partial:** installed desktop npm dependency license gate and negative tests now supplement the existing root Rust audit. Exact master `c44b0519580a5d52f230c20b0d29cee2ccee59af`; Rust CI [37963930935](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963930935), Platform CI [37963930918](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37963930918), both passed. Android Gradle/Tauri separate-dependency audits, stable dependency lockfiles and intentional-failure CI exercises remain open.
- **SRC-001 partial:** source manifests reject Windows-reserved path identifiers and ASCII-case-colliding member filenames. Exact master `1fe5d04d1c55039df46bc5f75d32d00ed2a37aff`; Rust CI [37964304931](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37964304931), Platform CI [37964304951](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37964304951), both passed. Official completed-generation discovery, resumable byte transport, hash verification and atomic activation remain open.

- **AI-001 partial:** provider-independent Rust `LlmProvider` contract with capabilities, locality, typed streaming/cancellation/error events and fake-provider success/timeout/bad-response tests. `stream_selected` explicitly enforces on-device/local/cloud destination policy before invocation, including tests rejecting unauthorized LAN/cloud dispatch. Qualified exact master `f96f2baa82d1934134725462db08d53acc421e38`: Rust CI [37966081890](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966081890) and Platform shells CI [37966082054](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966082054), both passed. Native asynchronous I/O adapters, production provider models and independent endpoints remain incomplete; **AI-001 stays unchecked**. The egress policy is a partial AI-004/SEC-003 guard, not proof of zero network traffic in a real app.
- **MOD-002 partial:** revision-bound hard citation IDs remain distinct from soft reading-position anchors. A new `wiki-core::soft_anchor` capture/relocate API relocates identical normalized text across revisions, but fails closed on missing/ambiguous duplicate content and cross-article changes. Qualified exact master `f96f2baa82d1934134725462db08d53acc421e38`: Rust CI [37966081890](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966081890) and Platform shells CI [37966082054](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966082054), both passed. HTML bridge, persistent bookmarks/highlights and real updated-article relocation remain open; **MOD-002 stays unchecked**.

- **MOD-001 partial:** canonical `Article` now preserves namespace and title aliases, with Unicode acceptance and rejection of blank/case-colliding aliases. All known Rust fixture constructors and cross-article citation tests were updated. Qualified exact master `d8e248d9588353cfb3074cf5d49415ce285dbdb8`: Rust workspace [37966708732](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966708732) and Platform shells [37966708731](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966708731), both passed. This remains an in-memory v1 model; serialization roundtrips, ingestion, and renderer fidelity are unqualified.
- **RAG-002 partial:** `validate_grounded_answer` distinguishes substantive cited responses from explicit no-evidence abstention, rejecting substantive text without evidence, fabricated IDs, empty answers and abstention carrying hidden content/citations. Qualified exact master `4f3c5a6b9b5827031dece4ef98a3b7a41a8ad6ee`: Rust workspace [37966981045](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966981045) and Platform shells [37966981260](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37966981260), both passed. This verifies **citation provenance, not factual entailment**. Full model/RAG orchestration, rendered citation navigation and unanswerable-question evaluation remain open.

- **PACK-001 partial:** pack manifest origin metadata now rejects unsafe/ambiguous publisher and custom-definition identifiers (path traversal, Windows separators, colon, leading/trailing whitespace) with regression coverage. Exact master `0fbc563fbbc7582e80122cd239dafd8be2206a87`; Rust CI [37967215105](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37967215105) and Platform shells CI [37967215022](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37967215022), both passed. No publisher cryptographic verification/signature trust is implied; container framing, manifests/asset validation, signed publisher chains, and installer acceptance remain outstanding.

### 2026-10-09 — Further exact-head qualification, roadmap items remain partial

- **RAG-001 / STORE-004 partial:** Restored the complete wiki-search source after an accidental truncated edit, preserving the existing corpus-level lexical retrieval and exact revision-bound evidence. Exact master `780afd1260c6fc7628c790990a0c86eb4f65e32c` passed Rust workspace CI [37969698813](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37969698813) and Platform shells CI [37969698488](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37969698488). BM25/installed-pack index, offline rebuild and article rendering are still pending.
- **SRC-001 partial:** `scripts/verify_source_staging.py` checks a caller-supplied local source manifest against exact staged member byte lengths and SHA-256 digests. It rejects incomplete generation metadata, nonofficial-looking source URLs, unsafe/reserved filenames, case-colliding member names, symlinks, missing/truncated and corrupted files. Negative tests run in CI; usage and trust boundary documented in `docs/DEVELOPMENT.md`. Exact master `85439224896b296429f50ec8e9211010f8f69353` passed Rust workspace CI [37970311028](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37970311028) and Platform shells CI [37970310890](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37970310890). **Not yet complete:** authentic upstream manifest discovery, source ownership verification, resumable download, staged activation/rollback and end-to-end acceptance. A matching caller-provided hash is not publisher authentication.

### 2026-10-09 — Additional qualified offline-media and article-RAG slices

- **PACK-003 partial:** Shared Rust media completeness gate requires every *declared* offline image, diagram, plot, map, math resource, CSS/font and A/V preview to have a verified local object with matching digest/size; duplicate content hashes count once. Audio/video streams cannot masquerade as installed bytes and require a preview. Rust workspace CI [37971726302](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37971726302) and Platform shells CI [37971726380](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37971726380) passed at exact master `5ee3a61a944a9f10cab58e1d650c62c5bd161485`. **Not yet complete:** discovering *all* renderer dependencies, verifying media bytes in the object store, SVG/MathML sanitization, raster resizing/zoom, Android offline E2E.
- **AI-005/RAG-002 partial:** `wiki-ai::rag::answer_article` now retrieves bounded blocks from one exact validated article revision, constructs provider-neutral context, honors selected outbound locality policy, buffers streaming answers, parses model block-citation claims and rejects fabricated/stale/uncited responses. Empty retrieval returns explicit insufficient evidence without invoking any provider; fake-provider negative tests cover prompt provenance, cross-revision citations and cloud denial. Rust workspace CI [37973697103](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37973697103) and Platform shells CI [37973697100](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37973697100) passed at exact master `c6e67d7cd36d71ecc9ca7b9c34ea6f104b9d5298`. **Not yet complete:** real provider adapters, answer factual-entailment evaluation, chat persistence, rendered links and supported device/browser flows.
- **RAG-003 partial:** Introduced six synthetic labeled retrieval queries, cross-section query, Unicode, non-answer and revision-isolation test cases. Synthetic top-3 recall is asserted at 6/6. Rust workspace CI [37975367021](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37975367021) and Platform shells CI [37975367016](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37975367016) passed at exact master `a860f3d28954982f4210e5235362c71e23be5857`. This is a small synthetic smoke baseline, **not** representative Wikimedia retrieval, citation precision, answer support, or device-performance qualification.

### AI-001 — provider-neutral interface, streaming, cancellation and fake-provider acceptance

- Qualified master SHA: `5cccbd289ef234f694724e190d761230f7939138`. Rust workspace CI [37976320142](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37976320142) and Platform shells CI [37976320137](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37976320137), both passed with exact head consistency.
- Contract: `crates/wiki-ai/src/lib.rs` exposes UI-independent `LlmProvider` with capability and model enumeration, connection tests, typed errors, streaming event callbacks, cooperative cancellation and explicit locality/outbound restrictions. `crates/wiki-ai/src/rag.rs` composes that trait without importing Tauri/Kotlin/Android/UI types or provider-specific APIs.
- Tests: `cargo test --workspace --locked` includes fake-provider success, timeout, invalid response, cancellation, invalid request, outbound-policy and citation-stream orchestration cases. Rust CI also runs `cargo fmt --all -- --check` and strict Clippy.
- Scope: **AI-001 complete; AI-002 through AI-005 are separate and remain unchecked**. No actual Ollama, llama-server, OpenAI, Anthropic or Gemini transport is claimed, and no real-device/UI chat acceptance is claimed.

- **AI-005/RAG-001 partial:** Selection context now accepts only an exact retrieved evidence object validated against the current article revision; selected passages are included without query keyword overlap and remain bounded by a word budget. Invalid or stale selected evidence is rejected before provider invocation. Exact master `5cccbd289ef234f694724e190d761230f7939138` passed Rust workspace CI 37976320142 and Platform shells CI 37976320137. UI selection bridge, real provider, citation scrolling and end-to-end acceptance remain open.

### 2026-10-09 — Qualified section IDs and snapshot-switch primitive

- **MOD-002 partial:** `wiki-core::evidence::evidence_sections` enumerates deterministic nested section handles with project, page ID, exact revision and ordinal path; tests reject invalid article structure and verify revision changes cannot silently reuse section IDs. Qualified exact master `03cc7becfc47d60389eb8af6bbbbc7cb2a8a2cd9` passed Rust workspace CI [37976522749](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37976522749) and Platform shells CI [37976522779](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37976522779). Actual reader scroll/highlight anchor integration and full soft-anchor lifecycle qualification remain open.
- **STORE-003 partial:** `wiki-store::activation::ActiveSnapshotStore` implements an atomic pointer switch to a caller-prevalidated immutable snapshot; it fsyncs temporary pointer bytes, atomically renames the pointer, preserves previous snapshot directories and rejects missing/unsafe IDs, corrupt pointers and symlinked candidates. Tests cover old→new activation and failure paths. Qualified exact master `3a5cb4c43ea325dcd942cda961058fbb18d025a3` passed Rust workspace CI [37977538227](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37977538227) and Platform shells CI [37977538201](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37977538201). **Not complete:** this switch primitive assumes the caller has already verified all content and signatures; manifest presence is not verification. Full SQLite catalog, staged byte-integrity enforcement, interruption/disk-full fault injection, rollback UI and mutable-state isolation remain open.


- **SRC-001 partial (local transport):** Added `scripts/stage_local_dump.py` to import locally acquired official public-dump members against a completed discovery report, validating byte length and upstream SHA-1 before no-clobber promotion and emitting local SHA-256 receipts. Symlink, corrupted source, existing-final and interrupted-HTTP-partial cases are covered by offline unit tests. Exact master `a8b6c351038b2170967406a0e38506285a08e00a` passed Rust workspace CI [37981353557](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37981353557) and Platform shells CI [37981352921](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37981352921). **Remaining:** trusted publication identity, multi-file completeness, snapshot ingestion/activation, live Wikimedia acceptance and rollback; upstream SHA-1 is not a signature.


- **SRC-001 partial (complete local generation verification):** `scripts/finalize_public_dump_staging.py` validates the entire completed public-dump member inventory against upstream SHA-1/byte lengths and emits a deterministic SHA-256 manifest accepted by `verify_source_staging.py`, with atomic no-clobber publication and missing/corrupt/duplicate/symlink negative tests. Exact master `e77cf1e873a8a5e6cdee9821008dbe3e7caca766`; Rust workspace CI [37981635719](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37981635719) passed. Platform CI [37981635730](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37981635730) was still running when this note was added. The discovery report itself remains unauthenticated; source import and snapshot activation remain open.

### 2026-10-09 — Source publication revalidation, XML extraction and conservative pack trust (partial)

- **SRC-001 partial:** HTTP generation staging now revalidates the complete caller-provided report against a fresh, completed official Wikimedia HTTPS `dumpstatus.json` (and SHA-1 sums when needed) **before member I/O**. It rejects changed member sets, missing or forged digests and unfinished upstream jobs. Standalone `scripts/verify_public_dump_publication.py` supports preflight verification. Local offline staging remains intentionally network-free and does not authenticate a caller-provided report. Exact source implementation at `b53a6247809c77a16f0ab50022385519b9ef367f` passed [Rust CI 37986917485](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37986917485) and [Platform CI 37986917399](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37986917399). SHA-1 metadata and HTTPS are **not signed publisher authentication**. Full trusted publication pinning, import, activation and live Wikimedia acceptance remain pending; SRC-001 stays unchecked.
- **TRUST-002 / TRUST-001 partial:** `wiki-pack-format::trust` provides explicit, accessible trust labels for unchecked, failed and custom integrity-verified manifests, with negative tests for invalid manifests. Merely claiming `Origin::Official` can **never** display `Verified publisher`, even when objects are reported intact. No verified publisher classification is issued until an actual pinned-key signature verifier exists. Exact master `01d11f0e5be8e13030163f059d7e1e750216e340` passed [Rust CI 37987399250](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37987399250) and [Platform CI 37987399433](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37987399433). Signature trust, real object-byte gates, installer enforcement and desktop/Android trust surfaces remain pending.
- **SRC-003 partial:** `scripts/import_public_dump_xml.py` consumes a **complete locally verified source manifest**, streams public XML/XML.bz2, extracts project/page ID/namespace/current revision/timestamp/raw wikitext SHA-256 and unresolved redirect target without confusing contributor IDs with page IDs. It rejects doctypes, entity declarations, duplicate page IDs, multiple revisions, oversized pages, malformed XML and unsafe/truncated inputs; atomically publishes preliminary raw NDJSON without overwriting earlier output. Exact master `d095091aabd2d4118cdf2c4963b87dcc901f2c9f` passed [Rust CI 37987706850](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37987706850) and [Platform CI 37987706989](https://github.com/ekkus93/foundation-wikipedia/actions/runs/37987706989). **Not complete:** raw wikitext is not rendered or normalized canonical article HTML; template/Lua/math/reference processing, cross-member duplication checks and real-dump acceptance remain open. SRC-003 stays unchecked.

## Blockers requiring owner action

_None at initial creation._ If a blocker arises, record exact failing operation, evidence, why independent progress cannot continue, the specific permission/input/hardware needed and the next action. Technical evaluations, normal code defects and running CI belong in the task notes, not here.