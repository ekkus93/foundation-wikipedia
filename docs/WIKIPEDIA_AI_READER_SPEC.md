# Foundation Wikipedia — Product and Technical Specification

**Status:** Authoritative proposed design baseline; implementation pending  
**Version:** 1.0 — 2026-10-09  
**Repository:** https://github.com/ekkus93/foundation-wikipedia  
**Companion checklist:** [WIKIPEDIA_AI_READER_TODO.md](WIKIPEDIA_AI_READER_TODO.md)

## 1. Vision, priorities and normative terminology

Foundation Wikipedia is a **Wikipedia reader first**, enhanced by optional article-aware AI. The product must preserve the recognizable Wikipedia desktop/mobile reading experience. The AI and application-owned controls appear as an overlay. **Reading, normal navigation, search, bookmarks and offline content must remain useful when AI is disabled or unavailable.**

In this document **MUST** indicates required behavior, **SHOULD** a strong default requiring written justification to change, and **MAY** optional or deferred behavior. Screens, example sizes and dates are illustrative, not measured release claims. No requirement is considered implemented merely because it is described here. The accompanying TODO governs implementation completion and evidence.

### Product invariants

- Shared, UI-independent Rust core; separate desktop and native Android presentation.
- Wikipedia content authority is Wikimedia; mirrors/caches are just transport sources.
- Canonical article representation has revision-scoped provenance and supports reliable AI citations.
- Installing an offline article means installing its required text, figures, images, diagrams, tables, math, captions and attribution; offline does not mean text-only.
- Audio and video media payloads stream/fetch on demand when online; local metadata and preview images remain available offline.
- Official and user-created packs use a compatible format, but the **trust status is visibly different**.
- No silent fallback from a local model to a remote/cloud model.
- Data and app updates are independent and must not destroy the last working installed content.

## 2. Platform matrix and architecture

| Concern | Desktop | Android | Future iOS |
|---|---|---|---|
| Application UI | Tauri 2 + React + TypeScript | Kotlin + Jetpack Compose | SwiftUI |
| Article renderer | Tauri webview with Wikipedia-like HTML | Android WebView embedded in Compose | WKWebView |
| Core | Rust workspace | Same Rust via UniFFI/Kotlin | Same Rust via UniFFI/Swift |
| Floating chat | Movable, resizable desktop overlay | Compose bottom sheet | Native sheet later |
| Offline topic pack install/read | Required | Required | Future |
| Raw Wikipedia import and pack creation | Desktop GUI and CLI | **Not present** | Not planned |
| Inference | Remote, LAN, localhost | Remote, LAN and optional downloaded on-device model | Later |

The Rust core MUST NOT depend on Tauri, React, Kotlin, Android-specific APIs, Swift or browser UI state. Tauri commands and UniFFI exports are thin application adapters around the same use-case API. GUI and CLI pack creation MUST share the same library. The app does not need an always-running local HTTP backend; a future server can be added separately.

Suggested workspace boundaries:

```text
crates/
  wiki-model/        # Article, block, citation and manifest data types
  wiki-core/         # use cases, navigation, settings/domain rules
  wiki-source/       # Wikimedia adapters and source ingestion
  wiki-store/        # immutable content + mutable user data
  wiki-search/       # lexical retrieval and optional derived indexes
  wiki-ai/           # RAG orchestration and LLM adapters
  wiki-pack-format/  # pack encoding, manifest, verification
  wiki-pack-builder/ # deterministic selection and build engine
  wiki-ffi/          # UniFFI DTO bridge

tools/wiki-pack/     # CLI built on wiki-pack-builder

desktop/frontend/    # React/TypeScript
desktop/src-tauri/   # Tauri adapter
android/             # Gradle, Kotlin, Compose/WebView
```

All cross-platform interfaces must use stable versioned DTOs and coarse-grained async calls; UIs must not directly manipulate pack shards or bypass core integrity policy.

## 3. Initial release scope versus deferred capabilities

**v0.1 target:** online-first Wikipedia reader; desktop and Android reading/search; user bookmarks/history; expressive action hub with Chat, Bookmark, Offline and Settings; article-scoped AI with local/LAN/remote providers and verified block citations; install/export of packs built in desktop Pack Builder; official pack catalog/trust/update capability; Android pack import/download; offline article images/math; settings subpages; optional small on-device model after real-device qualification.

**Explicitly deferred:** iOS, collaborative notes/accounts/sync, whole-Wikipedia vector database, unrestricted autonomous web research, per-object patch distribution, universal full-resolution Commons mirror, bundled audio/video payloads, fully native Compose article rendering, and full built-in desktop LLM inference engine. Anything described as optional must not become an undocumented v0.1 blocker.

An incremental build can ship working vertical slices, but v0.1 completion means the full user journey and qualification gates in §22 pass, not merely crate scaffolding.

## 4. Wikimedia sources and authoritative provenance

### 4.1 Source hierarchy

1. **Authority:** official Wikimedia publications and their page/revision metadata.
2. **Preferred structured bulk adapter:** Wikimedia Enterprise Structured Contents snapshots where available and permitted. These are beta and require adapter isolation; they provide pre-parsed data, but their payload does **not** supply all content/categories/redirects needed for this project.
3. **Complementary official feeds:** official article-body/rendered HTML data, category links, redirects, page identifiers and other metadata from compatible Wikimedia APIs, production snapshots or dumps. Join by project, page ID and **exact revision**, never by title alone.
4. **Fallback path:** official public current-content dumps, with a separately qualified MediaWiki/Parsoid-compatible renderer for complex wikitext/templates/Lua/math. Simply dumping XML text into WebView is insufficient.
5. **Optional alternative import adapters:** e.g. ZIM, clearly marked with fidelity limitations and upstream provenance; these are not a separate Wikipedia authority.

Reference upstream documentation to revisit when coding:
- https://enterprise.wikimedia.com/docs/snapshot/
- https://enterprise.wikimedia.com/api/structured-contents/
- https://dumps.wikimedia.org/
- https://www.mediawiki.org/wiki/API:Main_page

A proprietary paid subscription MUST NOT be an unconditional requirement for building from authoritative Wikimedia data. If a preferred source requires account credentials, clearly document the public alternative and its processing/fidelity costs.

### 4.2 Download and import

The source manager must enumerate actually completed snapshots, show project/date/format/size, support local file import, use resumable transfers, respect rate limits, and verify upstream hashes where provided. Mirrors may accelerate transport but may not redefine the snapshot or bypass verification. Detect duplicates, deletions, moves, redirects, namespace selection, inconsistent source components, partial files and schema drift. A mismatch between visible HTML revision and evidence-block revision MUST fail validation rather than produce misleading citations.

Source imports occur into staging; successful verification/normalization produces a versioned canonical snapshot. Interrupted operations never mutate the active snapshot. Snapshot provenance includes complete source URLs and actual checksums, not inferred names or approximate dates.

### 4.3 Online article freshness

Article cache keys include page ID and revision. Fresh online fetches can coexist with older installed snapshots; UI distinguishes **online current revision** versus **offline installed revision**. There is no requirement to fetch every newer article behind the scenes. Offline mode never triggers hidden network requests for required assets.

## 5. Canonical logical Article model

Article identity is `(project, page_id)`; title is mutable. Content identity includes `revision_id`, revision timestamp and a content hash. Core types are ours rather than direct serialized Wikimedia payloads.

```rust
struct ArticleKey { project: String, page_id: u64 }
struct Revision { revision_id: u64, timestamp: String, content_hash: String }
struct Article {
    schema_version: u32,
    key: ArticleKey,
    revision: Revision,
    title: String,
    display_title: String,
    language: String,
    namespace: i32,
    wikidata_id: Option<String>,
    lead: Vec<Block>,
    sections: Vec<Section>,
    references: Vec<Reference>,
    links: Vec<ArticleLink>,
    media: Vec<MediaRef>,
    rendered_html: String,
}
enum PageRecord { Article(Article), Redirect(Redirect) }
enum Block {
    Paragraph(TextBlock), List(ListBlock), Table(TableBlock),
    Quote(QuoteBlock), Math(MathBlock), Media(MediaBlock),
    HtmlFallback(HtmlBlock)
}
```

Exact Rust names will be finalized through implementation tests. Articles retain structured sections, nested blocks, references, footnotes, infoboxes, captions, lists, math and tables while retaining sanitized high-fidelity HTML for rendering. Disambiguation pages are readable articles; redirects are separate records. Preserve text direction, Unicode and link relationships. Unusual templates must have a safe render fallback rather than disappear.

### 5.1 Evidence identity

Every AI-addressable block gets a deterministic **revision-scoped** `BlockId`, derived from project, page, revision and stable within-revision block position/anchor. Block references link to section path, original text, HTML range and applicable Wikipedia reference IDs. The app validates claimed AI citations against the blocks actually retrieved. A Wikipedia footnote in a paragraph is not equivalent to independently inspecting the external cited source.

Bookmarks, saved selections and reading positions use a **separate soft anchor** (section path/text hash/position) so they can relocate after revision changes; a persisted evidence citation must not silently resolve to a different paragraph in another revision. The visible HTML, retrieval blocks and citation map are derived from the exact same canonical revision.

## 6. Snapshots, catalog and serialization

Logical storage contract: immutable content objects, SQLite catalog, versioned manifests, derived indexes, and mutable user state stored separately. **Provisional** physical implementation: individually zstd-compressed CBOR or MessagePack article records in addressable shards, with SQLite offset/length/hash pointers and content-addressed media. Benchmark record codecs, shard sizing and random-access behavior before finalizing an ADR.

```text
data/
  sources/<verified-upstream-id>/
  snapshots/<snapshot-id>/
    manifest.json
    catalog.sqlite
    articles/*.pack
    media/<hash-prefix>/<object-hash>
    search/               # derived; rebuildable
    ai/                   # derived chunks/vectors; optional
  current                 # atomic active-snapshot selection
  user-state/user.sqlite  # history, chats, bookmarks, settings
```

Catalog entries include project/page/title/normalized title/revision, record location, object hash, size, flags and references. Manifests include project/language, upstream identity, feature flags, schema version, counts, object hashes/lengths, build-tool version, original source and license metadata, signature/origin when relevant. No enormous single decompression stream is required to fetch one article.

Atomic activation/rollback, on-disk integrity checks, crash recovery and schema-version rejection are mandatory. App upgrades and Wikipedia-data upgrades remain independent. Derived search and embedding indexes can be discarded/rebuilt without reimporting article content. User state cannot live in an immutable pack.

## 7. Portable `.wpack` format and installation

A `.wpack` is a portable, versioned installable artifact containing a manifest, object references and required contents, index metadata, provenance and attribution. Its exact archive/container format is an evaluation item; it must support safe streamed import on Android, hashes, deterministic manifest serialization, bounded resource use and cross-platform fidelity.

Installation verifies format compatibility, manifest signature policy, object hashes, required media, index consistency and disk availability, stages objects, and **atomically activates** the installed pack. The installed store deduplicates identical article/media objects across packs and individual saves, regardless of whether transport archives include redundant bytes. Removing a pack releases only objects with no remaining owner. Cached articles are evictable; explicitly saved offline articles are not.

## 8. Official packs, custom packs and visible trust

- **Official packs:** curated/built and published by the project, discoverable in a verified catalog, signed by a recognized publisher key, versioned against a specific Wikimedia source snapshot, eligible for official update notification/download.
- **Custom packs:** created using desktop Pack Builder or CLI, saved as editable **definitions** separately from built immutable artifacts, importable on Android, locally integrity checked but **never labeled official merely because hashes match**. Rebuild against a new snapshot on desktop; transfer new artifact to Android.
- **Untrusted or invalid:** recognized as such in *every* pack card, details and install/import confirmation; invalid claimed signatures, object mismatch or unsafe format **block installation**. An unrecognized signing key does not establish official trust.

Visible statuses: **Verified publisher**, **Custom — integrity verified**, **Not yet checked**, **Verification failed**. Use labels/text/icons and accessibility descriptions rather than color alone. Trusted signatures prove origin under pinned-key policy; they are not permission to execute HTML, script or other files. Publisher-key distribution/rotation and signed-catalog details are release engineering evaluation items.

## 9. Topic-pack content selection

Definitions select Wikimedia project/language, curated topic roots, Wikipedia categories, recursion policy, specific page inclusions/exclusions, and optional bounded linked-prerequisite expansion. Category graphs are not clean taxonomies: detect cycles, administrative categories, topic fan-out, inappropriate nested categories and duplicate articles. Use a deterministic fixed sorted page-ID set per pack build. Include relevant redirects/disambiguation handling. Preview unexpected expansions before downloading media. Source and definition hashes make repeated builds reproducible.

Initial validated official topics SHOULD start with Mathematics, Physics and Computer Science; add others based on coverage/storage evidence. Users MAY create composite topics, e.g. Math + Science + CS, with explicit additions/removals. A pack's reusable definition is distinct from the versioned output and must survive a rebuild.

## 10. Mandatory offline images; A/V on demand

**Offline-ready means article text + every essential visual reference is local**: diagrams, photos, maps, scientific plots, tables, infobox visuals, equations/MathML/SVG, captions, footnotes, and attribution. All required HTML/CSS/font/math resources must also resolve without an Internet connection. SVG should remain scalable when safe; raster images should be optimized for phone storage without making labels illegible. Pinch/zoom for detailed visuals is required. Missing required visual objects fail the pack's offline completeness test.

Audio/video content bytes are **online on demand**: store title, caption, thumbnail/still image, duration when known, attribution, original source URL, license and description. When offline the article shows a meaningful non-broken preview and explains that a connection is required. Streaming starts only after user action and is subject to cellular/Wi-Fi policy; do not silently download permanent copies of large A/V payloads. Save-media-offline is deferred.

Content-addressed media assets have hash, MIME, dimensions, variant sizes, source, creator, license and attribution text. Reuse files across related packs. An article resource handler (Tauri and Android WebView local origin) maps logical media URIs to verified local objects rather than allowing arbitrary filesystem access.

## 11. Reader appearance, links and selection

Wikipedia article presentation is familiar, restrained and content-first, with typical desktop contents, headings, references, reading width and mobile behaviors. Use M3 Expressive only for app-owned UI chrome. Internal Wikipedia links are intercepted into app navigation; external links follow a documented system-browser policy. Search covers online results and clearly marked locally available results; installed packs and cached articles must not be conflated.

Use a narrow validated WebView bridge for selected text + block/revision IDs, safe anchor navigation, citation highlighting and local resource resolution. Add contextual “Explain”, “Explain simply”, “Why does this matter?” and “Ask about selection” actions without replacing ordinary Copy/Select semantics. The HTML itself cannot invoke privileged filesystem, secrets or arbitrary network functions. A citation click scrolls to and briefly highlights the exact supporting block; mismatched revisions produce a safe explanatory state.

## 12. Floating action hub — normative interactions

On every normal article-reading screen, a fixed **bottom-right main FAB** opens a vertical, labeled M3 Expressive speed dial. Its action order is stable **nearest main FAB: Chat, Bookmark, Offline, Settings**. Tapping main or outside/Escape closes it; selecting an action collapses the menu. Restore focus; provide keyboard access, large accessible touch targets, screen-reader labels, reduced-motion behavior and contrast. Additional actions may be registered in future (History/Article Tools), but should not clutter v0.1.

### Chat

Desktop opens a hovering movable/resizable/minimizable window constrained to viewport. Android opens a native Compose bottom sheet with peek/half/full states. Both show current article, selected model/locality, streaming/cancel and clickable citations. Closing chat doesn't delete its article-scoped conversation. Navigating to a different page switches to that article's history instead of silently combining contexts. Text selection populates relevant context.

### Bookmark

One tap adds/removes current logical article bookmark with confirmation feedback (no modal). Persist title, project/page ID, saved time, reading position and last-known revision. Reopening targets latest available content, relocating position when possible, with a notice if content was removed/redirected.

### Offline

Display article status: **pack-owned**, **explicitly saved**, **cached only**, or **online-only**. Saving the article includes all required visual media and metadata and shows measured size/progress. Removing one saved article must not remove objects owned by an installed pack. Manage pack controls are available here or in Wikipedia & Offline Data.

### Settings

Open as temporary app destination, preserving current page ID/revision, tab/back context, scroll position, expanded sections, chat open/minimized state and desktop window size/location. Leaving Settings returns **to the same wiki article at the same reading position**. Settings screens do not need an overlapping FAB.

## 13. M3 Expressive UI language

Article remains Wikipedia-like; app controls use expressive type hierarchy, varied shape families, tonal surfaces, clear primary/secondary emphasis, adaptive layout and springy responsive motion. Uniform generic rounded rectangles alone are not sufficient. Respect Android system dynamic color where available, legibility, orientation, accessibility, reduced motion and small displays. Desktop adopts expressive controls while retaining desktop pointer/keyboard affordances. Concept images are inspiration, not test fixtures.

## 14. Settings IA and subpages

Desktop: left category navigation plus detail region. Android: Compose category list and standard subpage navigation. Avoid a one-page settings dump. Only show controls that make sense on the active platform.

| Page | Controls and behavior |
|---|---|
| **General** | Wikipedia edition/language, startup/restore, link handling, desktop tab behavior |
| **Appearance & Reading** | System/light/dark, text size/width/font, section behavior, FAB visual/motion preferences |
| **AI Assistant** | Enable, provider profiles, model, endpoint/key, test connection, article-vs-Wikipedia scope, answer length, citations, streaming, fallback consent |
| **Wikipedia & Offline Data** | Installed source/pack versions, update checks, catalog, official/custom pack trust, topic management, desktop source and pack builder entry |
| **Storage** | Measured category usage, cache, packs, indexes, chats, free space, cleanup; desktop directory control |
| **History & Saved Content** | Bookmarks, chats, reading history, reading position, viewing/deleting saved data |
| **Privacy & Network** | Provider data disclosure, telemetry policy, Wi-Fi/cellular and media streaming rules |
| **Advanced** | Retrieval/index diagnostics, rebuild/validation, source adapters and developer diagnostics; hidden by default |
| **About** | App/core/schema versions, contributors, Wikimedia/media attribution and open-source licenses |

Normal users should not see BM25 tuning or ANN parameters unless Advanced/Developer features are deliberately enabled. Secret credentials live in OS-secure facilities, not ordinary settings files, logs or exported pack definitions.

## 15. LLM architecture and provider profiles

Shared Rust `LlmProvider` interface: supported-model enumeration where available, provider capabilities, streaming, cancellation, connection test, typed errors and token/context budgets. Provider-specific adapters for Ollama, generic OpenAI-compatible endpoints (including llama-server), hosted OpenAI/Anthropic/Gemini and later embedded runtimes. Do not assume all hosted providers share one identical protocol.

**Classify locality correctly:** on-device, localhost, local-network server, or remote cloud. A phone using a LAN PC LLM is not offline. Allow multiple saved provider profiles and easy switching. Default fallback is **none/ask first**; local unavailability must never cause an unconsented cloud request. The selected provider and data destination must be clear in chat and Settings. RAG never depends on the provider type.

The model manager should allow a **downloaded**, not base-APK-bundled, compact Q4-class model (initial evaluation target: Qwen3.5-0.8B; exact artifacts/license/compatibility to verify). Optional ~1.7B-class upgrade later. Assess Android llama.cpp-class vs accelerator runtimes using real-device RAM, startup, tokens/sec, context, thermal, binary size and robust cancellation. Inference OOM must fail gracefully while the Wikipedia reader keeps working. Suggested initial context budget 4K–8K pending benchmarks.

## 16. RAG, search and citation verification

**Default scope is the current article**. Rust chunks the exact article revision into provenance-linked section/paragraph blocks, retrieves the best with lexical + structural signals (section title, current location, selected text), caps context tokens, calls the selected model and validates returned citation IDs. Unanswerable questions should be identified instead of filled with hallucinated facts. User-selected cross-article/Wikipedia scope can be implemented separately and clearly labeled.

Prototype should use BM25/structural lexical retrieval, potentially Tantivy for local pack search. **No mandatory vector database.** Embeddings and rerankers are optional *derived* indexes, rebuildable independently of canonical articles. Measure benefits against a labeled question set before adding mobile storage requirements. The model must not manufacture links or cite blocks it never received. Track exact source revision in saved conversations.

## 17. Desktop Pack Builder UI/workflow

Pack Builder is a **top-level desktop area** (not buried in Settings) with **Installed, Official Packs, My Packs, Create Pack, Source Data** destinations. The GUI and `wiki-pack` CLI invoke the same Rust service. All progress and logs are durable, cancellations safe, and work may continue while user navigates elsewhere in the desktop app.

A guided wizard (M3 Expressive controls) runs:

1. **Basics:** pack name, wiki edition, source snapshot.
2. **Content:** curated Mathematics/Physics/Computer Science topic tiles; add categories/pages; advanced explicit include/exclude and bounded depth.
3. **Selection review:** resolved article count, redirect handling, category fan-out warnings and inspection before media download.
4. **Offline media policy:** mandatory images/diagrams/math; A/V available online on demand with local previews; not a checkbox to disable essential images.
5. **Search and AI data:** lexical index/RAG chunks; optional semantic index after qualification.
6. **Estimate:** counted articles/media, source and additional download size, installed size, dedup savings, free disk space; **no fake hardcoded estimates**.
7. **Build:** progress from selection through media acquisition/optimization, indexes, pack verification and export; safe restart and actionable errors.
8. **Complete:** Inspect, Install locally, Export `.wpack`, Save/Edit definition, Rebuild from new snapshot.

The reusable definition and built artifact are separate products; rebuilding cannot rewrite its source definition without user request. Display trust status and origin on all library cards and detail screens.

## 18. Android behavior and install experience

Android app is Kotlin/Compose with WebView only for article body. It can search, read online and offline, show the FAB/chat, use local/LAN/remote LLM profiles, import custom `.wpack` via system document picker, browse/install/update official packs, validate/remove packs and read articles with all visuals locally. **It cannot import raw Wikimedia snapshots or build packs.**

Display actual pack download size and storage requirements, official trust state versus custom-integrity state, and offline media requirements. Allow safe background/resumable downloading subject to OS policies. When a link points outside installed packs, fetch/cache if online, show “not installed” offline without breaking history. Audio/video preview is available offline; online playback follows data/Wi-Fi policy.

## 19. Publisher catalog and update strategy

The content publisher may offer official prebuilt packs; end users may build their own. Both use the same canonical `.wpack` format and source identification. Target an approximately **monthly** official pack refresh when a qualified authoritative Wikimedia snapshot is available, not a fixed-date guarantee. The exact hosting/CDN and publisher-key infrastructure must be evaluated; avoid tying source authority to a hosting vendor.

**v0.1 update:** whole-pack download replacement is acceptable. Stage, verify, activate atomically and keep previous working version for rollback. Existing content-addressed objects can be reused; pack-delta transport is deferred. Custom pack updates: desktop user selects newer verified source, rebuilds from saved definition, exports updated artifact, and optionally transfers to phone; Android verifies it as custom. Automatic checking/notification configurable, automatic download restricted by consent and connectivity policy.

App binary updates and Wikipedia data updates are separate processes. A new app cannot erase old packs without an explicit compatible migration; schema incompatibility is clearly reported. Failure of downloads, checksums, signature, compatibility, indexes, or storage does not damage installed content.

## 20. Persistence

Store mutable state in a separate SQLite database with migrations: bookmarks, history, reading positions, session/tabs, article-scoped chat messages and citation revision, provider profile nonsecret metadata, settings, installed-pack inventory, offline individual saves, download/build jobs and validation results. API keys and sensitive credentials use OS secure stores. Users can disable or delete history/chats without touching canonical article data. Conversations from old article revisions display provenance or stale-revision notice instead of silently mixing revisions.

## 21. Security, privacy and attribution

Treat upstream article HTML, model responses and imported archives as untrusted input. Require robust HTML/SVG/MathML sanitation, narrow WebView bridge with validated origin/messages, safe resource URI handlers, no untrusted privileged JS, defensive URL handling, archive traversal and decompression bomb limits, input quotas and integrity verification. Signing is origin verification, not exemption from validation. Track tampered/unrecognized publisher keys separately from valid unsigned custom packs.

Cloud inference sends only user-approved question/context/history to the selected endpoint and makes that disclosure visible. No hidden telemetry or cloud fallback. Store secrets securely and redact from diagnostics. Where A/V streams from external sources, disclose remote URL/network use in UI policy.

Provide article revision/source URL, contributor attribution/license reference, and per-media title/creator/source/license/attribution. Pack export must retain attribution metadata. Verify actual article and Commons asset redistribution/license requirements during release review. Do not imply Wikimedia Foundation endorsement or misuse its trademarks.

## 22. Qualification, performance and release gates

Build deterministic fixtures spanning short/long articles, math, nested tables, images/SVG, infobox, citations, redirects, category cycles, markup oddities, Unicode and version mismatch. Gate source fidelity, pack determinism, dedupe, crash recovery, signature tampering, unsafe-import rejection, chat citation accuracy, no cloud fallback, reader fidelity, M3 Expressive accessible controls and Android real-device behavior. Measure pack sizes, RAM, startup, article render/search latency, token rates and thermals on supported hardware before fixing performance budgets.

**End-to-end release acceptance** includes: desktop source import → create science pack → all mandatory images downloaded and attributed → verified `.wpack` export → Android import with **Custom — integrity verified** status → airplane mode → article text, diagrams, equations, search, citations and local model response (if model installed) work without unexpected network fetch → online audio/video plays only on demand → Settings/back restores exact reading position → failed update leaves prior content working. Separately validate signed Official pack shows **Verified publisher**. Record exact master SHA, CI results, artifacts, real-device steps and license audit.

## 23. Technical evaluations and decision gates

These choices are deliberately unresolved; their **required behavior and evaluation criteria** are specified above. Do not prematurely claim them decided.

| Open evaluation | Evidence required |
|---|---|
| Structured/HTML/category/redirect revision-matched acquisition | Real snapshot parity, availability, cost, missing fields, reproducibility |
| Public-dump rendering fallback | Templates/math/infobox/citation fidelity, licensing and hosting feasibility |
| CBOR versus MessagePack, shard layout | Measured compactness, random access, compatibility and Android memory |
| `.wpack` container and catalog signing | Safe stream extraction, canonical manifest, tamper resistance, Android import |
| Official hosting, mirrors and catalog | Secure discovery, range/resume support, bandwidth and publishing costs |
| Curated category rules | Bounded useful topical coverage and reproducibility |
| Android model/runtime | Real-device speed/RAM/thermal/citation QA/license compatibility |
| Offline image resolution policy | Visual readability and zoom against actual science/math diagrams |

## 24. Development and completion policy

Use [WIKIPEDIA_AI_READER_TODO.md](WIKIPEDIA_AI_READER_TODO.md) as the **sole canonical checklist**. Re-read current `master` at every development run. Work directly on `master` as requested, without creating a branch or PR per task; respect branch protection. Complete coherent vertical slices and run exact-head CI/tests before marking items complete. Preserve a clear evidence log and separate genuine user blockers from routine implementation/test problems. Neither this spec nor a mockup is proof that functionality has been implemented.