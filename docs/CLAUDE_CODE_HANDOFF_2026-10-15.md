# Claude Code handoff — Foundation Wikipedia

**Prepared:** 2026-10-16 UTC (2026-10-15 Pacific)  
**Repository:** https://github.com/ekkus93/foundation-wikipedia  
**Primary branch:** `master` (work directly on `master`; do **not** create a per-task branch or PR)  
**Pre-handoff baseline HEAD:** `fefaecb8b42bddf21adce3146beb09902b29717d`  
**Baseline commit message:** feat: flag unresolved HTML base URL semantics in visual diagnostics  
**Status:** **In development; not a production-ready Wikipedia reader or offline pack builder.**

> **Reading this handoff:** The baseline SHA above is the exact remote `master` commit examined before the handoff itself was committed. Creation of this file advances `master`, so re-fetch and inspect the actual latest HEAD before doing anything. Other Ralph/Claude sessions may also update `master` concurrently. This is a point-in-time handoff, **not** the authoritative roadmap; the specification and canonical TODO always take precedence.

## 1. Where we are now

Foundation Wikipedia aims to deliver a Wikipedia-first desktop and Android reader backed by a portable Rust core, accurate revision-scoped citations, optional grounded AI, and installable topic-oriented offline packs containing **all essential visuals and attribution**. Audio/video payloads are streamed on demand online; their previews and metadata should work offline. Official signed packs and integrity-verified custom packs must have clearly different trust status. Reading must work without AI.

Architecture: nine shared Rust library crates under `crates/` (`wiki-model`, `wiki-core`, `wiki-source`, `wiki-store`, `wiki-search`, `wiki-ai`, `wiki-pack-format`, `wiki-pack-builder`, `wiki-ffi`); `tools/wiki-pack/` CLI; desktop Tauri 2 / React / TypeScript; Android Kotlin / Jetpack Compose / restricted WebView. The shared Rust core must not depend on desktop/mobile UI frameworks.

**Qualified top-level canonical TODO items:** `BOOT-001`, `BOOT-002`, `BOOT-003`, `MOD-001`, `MOD-002`, `MOD-003`, `SRC-001`, and `AI-001` are checked. Many additional subcomponents have qualified *partial* slices but their parent requirements remain open. No other top-level item should be treated as complete merely because a library module or test exists.

**The earliest major uncompleted items** are `SRC-002` and `SRC-003` (official data joins and public dump fallback); downstream `SRC-004`/`SRC-005`, `STORE-*`, `PACK-*`, end-to-end reader, Android, trust, RAG and release work remain unfinished. `README.md`/`docs/DEVELOPMENT.md` describe largely bootstrap UI shells, not a working user product. There are partial online HTTP adapters, cache/FTS functionality, media staging, model, search, and AI provider modules, but do not equate these to end-to-end acceptance.

## 2. Authoritative references and important files

Read these *from the current HEAD*, in this order:

1. `docs/WIKIPEDIA_AI_READER_SPEC.md` — authoritative product and technical requirements (315 lines at baseline).
2. `docs/WIKIPEDIA_AI_READER_TODO.md` — **sole authoritative completion checklist**, 1,536 lines at baseline. Paginate reads; never replace the whole file with the first bounded excerpt.
3. `docs/DEVELOPMENT.md` — real host setup, test commands, Android native stub and source staging workflows.
4. `docs/SRC_003_CENSUS_PIPELINE_2026-10-14.md` — staged dump/census implementation, limits, and qualification history.
5. `docs/SRC_003_VISUAL_CANDIDATE_DEDUP_2026-10-15.md` — renderer asset diagnostics, most recent CSS/MathML/SVG work and explicit non-claims.
6. `docs/SRC_002_SCOPED_IMPORT_QUALIFICATION_2026-10-14.md` — structured source joins, digest pins, deletions and remaining authenticity limitations.
7. `docs/RALPH_BRIDGE_WRITE_AND_CI_INCIDENTS_2026-10-15.md` — cumulative, **append-only** incident and qualification evidence.
8. `docs/SRC_003_RENDERER_EVALUATION_2026-10-14.md`, `docs/SRC_004_HOST_HTTPS_TRANSPORT_2026-10-13.md`, relevant ADRs and package tests before changing source/renderer behavior.

**Protected original documents:** `docs/WIKIPEDIA_AI_READER_TODO.md` is **1,536 lines**, original Git blob SHA `ec594ed49cf4fff5e1c7ab42fca927881be55df5` from source commit `007b16e023ce30e31851216ce147e2dc6c1c4dbc`, unless later complete, intentional edits properly extend it. `docs/SRC_003_CENSUS_PIPELINE_2026-10-14.md` is **346 lines**, blob SHA `0191d49981243930fce5039d03225248068ac51d`. Both were verified at the pre-handoff HEAD and should be preserved. If the canonical TODO is accidentally truncated, restore the *entire* correct version before any further development. For Ralph Bridge, use `restore_file_from_ref` against an exact observed master head where appropriate.

## 3. Exact-head qualification at handoff

Pre-handoff `master`: `fefaecb8b42bddf21adce3146beb09902b29717d`, **CI aggregate: passed**.

| Workflow | GitHub Actions run | Conclusion |
|---|---|---|
| Rust workspace | [38851595375](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38851595375) | success |
| Platform shells | [38851595323](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38851595323) | success |
| Renderer dump qualification | [38851595224](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38851595224) | success |

Those runs qualify automated behavior **at that commit**, not full product readiness, device acceptance, Wikimedia source authenticity, or completeness of media licensing. Revalidate CI after every source or documentation commit.

## 4. Recent work: precise current stopping point

The immediately preceding development was focused on bounded **rendered-media URL discovery diagnostics**, contributing toward `SRC-003`/`PACK-003`; it did **not** implement a production-ready offline asset downloader.

Relevant code:
- `scripts/inventory_rendered_visuals.py` — diagnostic HTML inventory and `inventory_css_dependency_graph` for *caller-supplied* external stylesheets.
- `scripts/test_inventory_rendered_visuals.py` — deterministic regressions for candidate sets, responsive images, CSS, MathML and malformed input.
- `crates/wiki-pack-builder/src/media.rs` and `crates/wiki-pack-format/src/media_coverage.rs` — declared media/object preflight, **not** renderer-complete media certification.
- Renderer CI: `.github/workflows/renderer-dump-qualification.yml`.

**Already qualified improvements (selected evidence):**
- SVG `feImage` external candidate detection and conflicting `href` / `xlink:href` fail-closed guards at `50521259e6f137efedda463dc9696cc9f0d75cbd`, exact-head CI Rust `38846483450`, Platform `38846483456`, Renderer `38846483461`.
- MathML `mglyph` references and flags for unresolved `canvas`, `iframe` and SVG foreign-object surfaces at `e7199187db7783bd0d1d171df41c09410d965c61`; Rust `38846881723`, Platform `38846881757`, Renderer `38846881716` passed.
- Bounded external stylesheet URL graph handling `@import` chains, relative paths, cycles, missing supplied imports, SHA-256 candidate fingerprinting, and resource limits. Final source repair `9ef44704efe8b53e0c4dbccda35988c76988a895` passed Rust `38847968807`, Platform `38847968857`, Renderer `38847968811`.
- Qualification and incident logging committed at `ab7f856b531856a1c07eacbbf628e132abadeba1`; Rust `38848234083` and Platform `38848234064` passed.

**Current stopping point:** the most recent remote HEAD `fefaecb8b42bddf21adce3146beb09902b29717d` is a `Restore file from exact repository ref` commit. Inspection of its diff against its parent showed a change **only** in `scripts/test_inventory_rendered_visuals.py`: removal of some incomplete/experimental HTML `<base href>` and duplicate-attribute assertions (plus blank lines). This is **not** proof that HTML base-URL handling is complete. Inspect actual source and current tests before resuming and do not reintroduce failing expectations without a corresponding implementation. Base-URL ambiguity remains an unqualified area.

**Known limitations of current media diagnostics:** no network transfer; no independently authenticated source bytes; no fully compliant HTML/CSS tokenizer; no complete recursive font/media resolution; no Commons source/creator/license acquisition; no verified installed object closure; no Android airplane-mode acceptance. The flags `css_and_font_dependencies_verified=false` and `required_media_verified=false` must stay fail-closed until genuinely satisfied. Never mark `SRC-003` or `PACK-003` done from these diagnostic improvements alone.

## 5. Prioritized remaining engineering work

Use dependency-first judgement and avoid duplicating existing partial implementations. Re-read the TODO's actual checkboxes and latest module code.

### P0 — source fidelity / publisher-grounded bulk ingestion

1. **`SRC-002`**: complete end-to-end authenticated Wikimedia Enterprise/companion joins where available; verify independent publication identity and completeness, deletion-feed coverage, project/page/exact-revision consistency, rendered HTML/category/redirect/body provenance and failure isolation. Existing `wiki-source` partial pinned NDJSON/join/deletion modules provide a start. Do not require a paid Enterprise subscription: official public dumps must remain viable.
2. **`SRC-003`**: complete official public current-content dump importer (prefer shared portable Rust path), independently validate official snapshot/member/checksum/census expectations, and qualify a MediaWiki/Parsoid-compatible renderer for templates, Lua, MathML, references, infoboxes, and difficult page fixtures. Existing Python dump/bzip2/XML census and renderer CI code are diagnostic/partial. Record real-source parity and fidelity/hosting tradeoffs; avoid claiming raw wikitext is browser-ready HTML.
3. **`SRC-005`**: expand authoritative-like golden regression fixtures for redirects/deletion/revision changes, science/math/media, tables/footnotes and non-English content. Build end-to-end tests before checking off parent items.

### P1 — truly complete visual dependencies and portable offline packs

4. **`PACK-003` / `STORE-002`**: move from URL *candidates* to full required-resource discovery. Resolve rendered HTML, scoped CSS + external imports, fonts, images/srcset, SVG/MathML, diagrams, captions and A/V preview assets; classify unresolved dynamic surfaces as blockers. Authenticate source/Commons metadata, acquire and hash bytes, sanitize visual formats, store per-media title/creator/license/source/attribution and verify installed resolution offline. Enforce provenance and completeness; no "offline ready" label until every mandatory visual is local and verified.
5. **`STORE-001` / `STORE-003` / `STORE-004`**: finish persistent SQLite/shard catalog, crash-safe activation/rollback, corruption/index rebuild, user-data separation, deduplication/ownership/GC and measured offline search.
6. **`PACK-001`/`PACK-002`/`PACK-004`/`PACK-005`/`PACK-006`**: finalize a versioned stream-safe `.wpack` format and import/export validation; deterministic category/topic resolution and preview; source-to-artifact/CLI pipeline; durable builder jobs; curated Math, Physics and Computer Science definitions. A provisional CLI/staging library exists but does **not** demonstrate final end-to-end installation.
7. **`TRUST-001`/`TRUST-002`/`TRUST-003`**: publisher signatures, custom-integrity-vs-official trust labels and hostile input checks. Never mark unsigned custom packs "Verified publisher."

### P2 — actual user journeys and release gates

8. **`SRC-004`/`DUI-001..003`/`STATE-001..003`/`UX-001..004`**: wire revision-pinned online source fetching and installed article lookup into the desktop reader; implement search, safe media/local URLs, normal navigation, selection/citation highlight, global bottom-right action hub, chat/bookmark/offline/settings and return-state behavior.
9. **`RAG-001..004`/`AI-002..005`**: article-scoped lexical evidence, citation-ID verification and test corpus, local/LAN + cloud profiles, stream/cancel, privacy/secret handling; no silent cloud fallback or unsupported-answer citations.
10. **`MOB-001..004`/`MOBPACK-001..003`**: complete actual UniFFI Kotlin callable bridge, on-device article reader, Compose M3 Expressive UI and custom/official pack install. Airplane mode must show *all* mandatory images, diagrams, math, captions, search and citations without hidden network requests. A/V streams on explicit request when online.
11. **`LOCAL-*`/`UPD-*`/`SEC-*`/`QA-*`/`REL-*`**: qualify optional Android local LLM against real hardware, publisher distribution/update/rollback, security and licensing, accessibility/performance, desktop packages, APK/AAB and cross-device v0.1 acceptance. See SPEC `22 and roadmap Phase 18 for the release acceptance definition.

Most of these tasks remain unchecked. Keep the scope honest: *working tests for partial primitives are not evidence of finished user journeys.*

## 6. Development and qualification commands

Use `docs/DEVELOPMENT.md` for host prerequisites and toolchain details:

~~~sh
# Shared Rust workspace: pinned root Rust 1.85.1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p wiki-pack -- --help

# Renderer diagnostic unit tests
python3 -m unittest scripts/test_filter_dump_for_render.py scripts/test_compare_rendered_html.py scripts/test_inventory_rendered_visuals.py

# Desktop frontend: Node 22, Tauri native Rust 1.90.0
cd desktop
npm install
npm run lint
npm run build
cd ..
cargo +1.90.0 check --manifest-path desktop/src-tauri/Cargo.toml

# Android: JDK 17, Android SDK 35, Gradle 8.11.1
cd android
gradle :app:assembleDebug :app:testDebugUnitTest --no-daemon
~~~

Android native `buildRustArm64` and UniFFI code generation are currently opt-in scaffolding; normal APK assembly does not establish a working native bridge. Device qualification needs appropriate connected hardware/emulator and documented evidence. GitHub Actions outcomes must be checked against **the exact resulting commit SHA**, not merely the branch name.

## 7. Ralph Bridge / concurrency / incident discipline

- This ChatGPT workflow uses **Ralph Bridge exclusively** for all GitHub and CI reads/writes. The owner explicitly requests direct commits on `master`, no per-task branches/PRs. Coordinate any local Claude Code pushes with concurrent runs rather than racing unconditionally.
- Bridge runtime at inspection: **v0.6.2**, build `0acea4b23362e2ccecab8f6c9db4418b162426cf`, instance `2b818998d9ea4beab846a083bc4b3224`, started `2026-10-15T22:44:38.900088Z`, auth mode `app`.
- **Never assume a bounded file read returned the entire file.** Honor `next_start_line`/pagination and compare original/full blob identity before editing or restoring. The canonical 1,536-line TODO has previously been truncated by an incomplete read; preserve or explicitly restore it. Preserve the 346-line SRC-003 census as well.
- Before a direct-master mutation: re-read `master`; do a compare-and-swap write against its **exact full SHA**. After any uncertain response, re-read the branch and changed-file diff before retrying; successful writes can return uncertain outer acknowledgments. Avoid simultaneous writes to the same source/TODO/docs from separate sessions.
- Append non-sensitive new failures/qualification to `docs/RALPH_BRIDGE_WRITE_AND_CI_INCIDENTS_2026-10-15.md`, **without replacing earlier entries**. Record UTC times, Bridge runtime/build/instance/start/auth, operation ID, sanitized arguments, expected/observed SHA, acceptance/reconciliation, error/structured diagnostics, CI run/job/step IDs, bounded log excerpts, retry and confirmed resolution. Keep authored test/CI defects distinct from Bridge/API errors, outer safety denials and bounded wait expirations. Do not write credentials, tokens or unbounded logs.
- Recent examples in the incident log: ambiguous ref-update acknowledgment reconciled to successful commit `89e74cf1…`; two renderer failures caused by SHA fixture escaping and a two-backslash URL predicate, fixed by subsequent source/test commits; documented outer platform safety denials during the attempted HTML base-URL guard. None should be diagnosed generically as a Bridge defect without evidence.
- Master can move during lengthy inspection. If it moves, **rebase the intended change on the new content**; never blindly overwrite a concurrent commit or force-push. Reload TODO after each coherent slice/merge.

## 8. First concrete actions for Claude Code

1. Fetch latest `master`; inspect the new actual head, open branches/PRs, exact-head workflows, any concurrent edits and repo state. Use this document only as the starting checkpoint.
2. Verify both protected documents: canonical TODO 1,536 lines/original blob and SRC-003 census 346 lines/original blob, unless newer intentional edits fully preserve requirements. Read all relevant TODO windows before editing.
3. Run baseline Rust/renderer checks and inspect `scripts/inventory_rendered_visuals.py` plus tests. Verify the reverted/unimplemented HTML `<base href>` handling and decide an explicit fail-closed resource-base policy before reintroducing tests. Keep the candidate-vs-verified-media distinction.
4. Work on the earliest dependency-satisfied incomplete source fidelity/asset-completeness task, preferably producing an **end-to-end verified Wikimedia dump → rendered canonical article + complete media inventory** fixture rather than extending speculative URL heuristics indefinitely.
5. Finish one coherent slice, commit directly on `master`, qualify exact-head CI and any required real-device tests, document results in the appropriate qualification document and append incidents. Re-read current TODO, and continue. Only check a parent ID when *every* acceptance criterion and supporting evidence exists.

**No user-specific credential/hardware blocker was identified for the automated diagnostic work summarized here.** Real publisher signing/hosting, live Enterprise access where chosen, and Android real-device acceptance may eventually need owner input. Investigate usable public/fixture alternatives and continue independent work before raising such a blocker.
