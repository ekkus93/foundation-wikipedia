# Foundation Wikipedia — development handoff (2026-11-12)

**Purpose:** Resume development on a different computer without losing the active engineering context.
**Repository:** https://github.com/ekkus93/foundation-wikipedia
**Integration branch:** `master` only; do not create per-task branches or pull requests.
**Inspected code baseline (before this documentation commit):** `0ccd0bf153a45ac2913234a59bbf35f21e78a31d`.
**Authoritative requirements:** [WIKIPEDIA_AI_READER_SPEC.md](WIKIPEDIA_AI_READER_SPEC.md).
**Sole authoritative task/acceptance checklist:** [WIKIPEDIA_AI_READER_TODO.md](WIKIPEDIA_AI_READER_TODO.md).
**Build prerequisites and integration notes:** [DEVELOPMENT.md](DEVELOPMENT.md).
**This handoff is descriptive, not a replacement checklist or an assertion of acceptance.** Re-read the latest `master`, SPEC, entire canonical TODO and exact-head CI before coding.

## 1. Status at handoff

- **Project is in active implementation, not release-ready.** Shared Rust crates have substantial validated components, but the README explicitly labels desktop and Android applications **bootstrap previews**, not functioning complete Wikipedia readers. Do not equate passing infrastructure/core tests with the end-to-end product.
- **Most recent development focus:** Phase 3 `STORE-003` (staged snapshot activation, rollback, user-state isolation). Its two acceptance subtasks are **unchecked** in the canonical TODO: (1) atomic activation with retained old version and crash/disk-full fault injection; (2) separate mutable user SQLite and rebuildable search/embeddings. The acceptance scenario is that an interrupted activation leaves previous content **and bookmarks** intact.
- **Currently open technical target:** ensure the staged candidate manifest and required ancestor/directory metadata are durably synchronized **before** the active selector can be published; then qualify fault and recovery boundaries. This is a proposed improvement from the last review, **not yet implemented, tested or committed**.
- **Repository `master` at inspection:** `0ccd0bf153a45ac2913234a59bbf35f21e78a31d` (`fix(store): apply rustfmt correction to manifest link guard`). The four GitHub Actions workflows completed successfully on this exact SHA:
  - Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/43323345503
  - Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/43323345449
  - Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/43323345693
  - Android record codec probe: https://github.com/ekkus93/foundation-wikipedia/actions/runs/43323345714
- These CI links establish the **baseline only**; they do not establish CI on the later handoff-documentation commit. Always check runs for the new exact SHA.
- No owner-action blocker was confirmed by the canonical TODO's existing blocker register at inspection. Missing physical-device/power-cut evidence is a **qualification gap**, not evidence of a completed acceptance gate.

## 2. What is implemented in the active area

Primary file: `crates/wiki-store/src/activation.rs` (approximately 1,922 lines at baseline). Related components include `crates/wiki-store/src/offline_reader.rs`, catalog and FTS index, media object storage, media ownership/registry, and the media lease coordinator.

- `ActiveSnapshotStore::current` validates selector content, staging identity, path metadata and read identity; it rejects unexpected selector replacements and unsafe paths rather than silently adopting an untrusted pointer.
- `switch_to_prevalidated` and the shared internal switch path write a private selector file, use atomic selector replacement, preserve previous snapshot directories, and attempt a durably synchronized rollback if post-rename synchronization fails. If rollback durability cannot be established, the code surfaces `ActivationError::RollbackFailed`; callers must not report success or silently discard the old data.
- `switch_to_manifest_verified_with_bound_pack_media` validates the **selected candidate** under `snapshots/<id>`, binding its on-disk catalog, persisted search index and manifest-described shards to mandatory registered media verification under a shared media lease. Caller-supplied publisher-authenticated manifest and complete inventories remain prerequisites; the local digest checks **do not authenticate the publisher**.
- Staging checks reject absent or unsafe manifest paths, symlink substitutions and (on Unix) multiply hard-linked candidate manifests. The switch repeats staging checks near the commit boundary.
- Deterministic tests already exercise first activation, existing-selector preservation, failed sync/rollback, orphan temporary files, precommit rejection, substituted manifests/directories, and an invalidated staged candidate.

Selected recent commits (see their diffs rather than relying on this summary):

| SHA | Scope |
| --- | --- |
| `2f86a8b6c382a8f4f311f688c832549edd7ba3a3` | Bound catalog/shard/index/media validation to the staged candidate; repaired accidental source truncation. |
| `af7dc47eb274fa9bf2dbbd9caf85b340e3895327` | Recheck staging at precommit after candidate manifest removal; protect old active snapshot. |
| `f1cf5a0fc1d85d78fc19c9425fa604a8688ee396` | Reject symlinked candidate manifest in precommit regression. |
| `25264cb3ed88391f23fa5e3a63361b629f8563db` / `684182a53527b7e0cdf761b814dad2c300856332` | Candidate-directory symlink regression and formatting repair. |
| `15d37c53e7ef72b5676c5f5b5fb47b9ea907aa8e` / `0ccd0bf153a45ac2913234a59bbf35f21e78a31d` | Candidate manifest hard-link rejection and formatting repair; all four CI workflows passed at the second SHA. |

**Important scope boundary:** synthetic error injection and passing CI are **not** proof that actual power loss, ENOSPC, cross-filesystem behavior, noncooperating writers, or production Android/desktop installation paths are safe.

## 3. Next engineering work — concrete order

1. **Refresh and reproduce the baseline.** Fetch exact current `master` and complete canonical TODO, read the current activation/store source and `docs/DEVELOPMENT.md`, inspect exact-head CI, and run Rust format/Clippy/tests locally. If master moved, reconcile changes before making a compare-and-swap write.
2. **Implement a staged-data durability barrier (STORE-003).** Trace *who creates the candidate* and when its manifest, immutable content shards, catalog and derived search index are actually flushed. Before publishing `current`, establish explicit, correctly ordered file synchronization and parent-directory synchronization for the staged candidate and required directory entries. Cover creation and existing-file cases; avoid reporting durable success when a prerequisite sync fails. Consider supported-platform filesystem behavior and whether the transaction should be performed by the staging installer rather than by a read-only verification helper. Preserve immutable/source-authenticated content boundaries and exclusive ownership assumptions.
3. **Add deterministic fault injection exercising production recovery, not just guards.** Inject errors at candidate file sync, parent-directory sync, selector temp write/sync, atomic rename, post-rename directory sync, and rollback sync. Include ENOSPC-like failures when staging, persisting catalog/index, and switching; assert old content is still readable, bookmarks/user SQLite are untouched, orphan files are handled without adoption, and uncertain-durability errors are explicit. Fix any defects exposed, with tests that fail before the fix.
4. **Close user-state isolation acceptance.** Verify mutable SQLite lives separately from content snapshots, survives failed activations and successful switches, and that search/embedding indexes can be rebuilt independently of source/media redownload. Add disk-backed integration tests (including restart/reopen) instead of accepting isolated unit coverage.
5. **Qualify on target filesystems and devices.** Test true process interruption/power cut (or a documented reproducible crash/power-failure harness), low-space failure, snapshot upgrade/rollback, and reader/bookmark retention on desktop and Android hardware. Record OS/filesystem/device, exact SHA, commands and artifacts in the canonical TODO. Do not mark `STORE-003` complete until the required acceptance evidence exists.
6. **Do not stall on unavailable hardware.** If device/power-cut qualification is genuinely impossible on the new computer, record the precise missing resource and continue the next independently implementable TODO item. Never repeatedly re-review unchanged code instead of implementing a useful vertical slice.

For the activation implementation, start with `ActiveSnapshotStore::switch_with_precommit_and_sync`, `candidate_is_staged`, the candidate staging writer(s), and `switch_to_manifest_verified_with_bound_pack_media`. Inspect the exact current signatures; method details may have changed since this handoff. The current code itself explicitly says it only implements the atomic switch primitive and that caller validation/authentication is separate.

## 4. Remaining roadmap — high-level, not an invented completion percentage

The unchecked **top-level task IDs** at inspection include:

- **Source/reader:** `SRC-002` to `SRC-005`: authenticated complete Structured Contents joins, public dump rendering/fidelity, online cache/user journey, source-level fixtures.
- **Storage/search:** `STORE-001` to `STORE-004`: production catalog qualification, media owner lifetime/garbage collection, snapshot crash recovery and state isolation, offline search rebuild and device acceptance.
- **Packs/trust:** `PACK-001` to `PACK-006`, `TRUST-001` to `TRUST-003`: portable verified .wpack, topic resolution, **all mandatory offline figures/diagrams/math/captions/attribution**, builder jobs/CLI, signed official-vs-custom trust, hostile import protection.
- **Retrieval/AI:** `RAG-001` to `RAG-004`, `AI-002` to `AI-005`: reliable source citations and retrieval QA; local/LAN and remote providers, profiles/consent, conversation lifecycle. `AI-001` is checked.
- **Desktop/UI/state:** `DUI-001` to `DUI-003`, `UX-001` to `UX-004`, `PUI-001` to `PUI-005`, `STATE-001` to `STATE-003`: actual article/navigation UI, action hub, Pack Builder, persistent state/settings.
- **Mobile and publication:** `MOB-001` to `MOB-004`, `MOBPACK-001` to `MOBPACK-003`, `LOCAL-001` to `LOCAL-003`, `UPD-001` to `UPD-004`: production UniFFI/Compose/WebView bridge, Android pack lifecycle, optional on-device AI qualification, catalog/update distribution.
- **Release gates:** `SEC-001` to `SEC-003`, `QA-001` to `QA-003`, `REL-001` to `REL-003`: security/privacy/licenses, usability/performance, packaging and real-device end-to-end acceptance.
- `FUT-001` to `FUT-006` are **explicitly deferred** and should **not** block v0.1 unless requirements change.

Several unchecked parents contain already-implemented, CI-qualified sub-slices documented in the very large canonical TODO. Do not replace those statuses with a simplistic “not started”, and do not mark a parent complete merely because its individual helpers or regression tests pass. Checked top-level tasks at inspection are `BOOT-001..003`, `MOD-001..003`, `SRC-001`, and `AI-001`; reconcile against current TODO before reporting totals.

**Product acceptance invariant:** a desktop-built, Wikimedia-sourced pack with required offline visuals can be exported, verified/imported on Android, read in airplane mode with working search/citations and no silent remote calls, while failed content updates preserve old content and state. Official signed packs must show verified publisher trust distinctly from valid unsigned custom packs; optional A/V payloads are streamed on demand.

## 5. Bring up a different development computer

All exact prerequisite versions and OS packages are documented in [DEVELOPMENT.md](DEVELOPMENT.md); treat that as authoritative if these notes drift.

1. Clone the repository on the new host, switch to `master`, and fast-forward from `origin/master`. The handoff and TODO are committed files; do not rely on an old local clone or any untransferred working-tree edits.
2. Install root Rust **1.85.1** (pinned in `rust-toolchain.toml`) with `rustfmt` and `clippy`. The native Tauri adapter separately pins **Rust 1.90.0** under `desktop/src-tauri/`.
3. For desktop UI use Node.js **22**, npm and Tauri 2 Linux dependencies (WebKitGTK 4.1, Ayatana appindicator, librsvg, patchelf, OpenSSL dev libraries; see the exact Ubuntu package command in DEVELOPMENT.md).
4. For Android use **JDK 17**, **Gradle 8.11.1**, SDK platform **35**, build-tools **35.0.0**, minimum API **26**. Gradle wrapper artifacts are not yet checked in. Optional arm64 Rust native build needs NDK and `aarch64-linux-android` Rust target; current UniFFI generation is only a stub, not a working Kotlin runtime bridge.
5. Host-only online Wikimedia staging uses `curl` and OS CA trust; the Android app does not inherit this transport. Unit tests do not require Wikimedia credentials or actual bulk data.

Local smoke-check commands (from checkout root unless noted):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p wiki-pack -- --help

cd desktop
npm install
npm run lint
npm run build
# Native Linux Tauri after installing OS libraries:
npm run tauri dev
cd ..

cd android
gradle :app:assembleDebug :app:testDebugUnitTest --no-daemon
```

The desktop and Android commands validate scaffolding rather than finished reader functionality. Android debug APK location after successful build: `android/app/build/outputs/apk/debug/app-debug.apk`. See DEVELOPMENT.md for the opt-in NDK task and system-specific details.

## 6. Ralph Loop workflow and failure-avoidance notes

- **All assistant-initiated GitHub reads/writes/commits and CI operations must use Ralph Bridge exclusively**; never the default GitHub connector. Work directly on `master` with the exact **40-character expected-head SHA** compare-and-swap guard. Do not create a branch or PR for each subtask. Verify the new remote head and changed file(s) after each accepted commit. A successful `commit_files` or `create_or_update_file` operation updates the remote branch; no separate `git push` is needed by the assistant.
- Check exact-SHA CI (Rust workspace, Platform shells, Android Rust ABI, Android record codec). If running, distinguish pending from passing. Investigate failures, correct them and check CI again. One Android codec probe failure in this series was a transient ADB broken pipe on slow emulator startup; an exact-commit rerun passed. Do not automatically label every emulator failure transient.
- **Beware huge files and bounded Ralph reads.** The canonical TODO was ~923 KB / 2,276 lines and `activation.rs` ~1,922 lines at this baseline. `get_file` returns *bounded line windows*. Do **not** reconstruct and overwrite a complete file from a truncated response. Earlier repairs had to restore a missing `activation.rs` test tail. If changing a large source file, assemble and verify **all** windows (check `next_start_line`, total lines/bytes, missing tail), prefer small localized edits, inspect the actual diff and run rustfmt before committing.
- CI formatting failures have recurred after source reconstruction. Validate `cargo fmt --all -- --check` before a write when possible; follow up immediately if remote CI flags formatting errors. Respect concurrent master updates and do not blindly retry an ambiguous write.
- No TODO checkbox should be checked without implementation, automated coverage, relevant manual/device verification and an exact-commit evidence ledger entry. Keep genuine owner blockers separate from normal failures and next-step implementation decisions.

## 7. Suggested resume request

> For `ekkus93/foundation-wikipedia`, use Ralph Bridge exclusively for all GitHub and CI operations, work directly on master with exact-head compare-and-swap commits, and create no per-task branches/PRs. Read `docs/FOUNDATION_WIKIPEDIA_DEVELOPMENT_HANDOFF_2026-11-12.md`, `docs/WIKIPEDIA_AI_READER_SPEC.md`, the entire `docs/WIKIPEDIA_AI_READER_TODO.md`, latest `master`, relevant source and exact-head CI. Implement the next substantive `STORE-003` durability/fault-recovery vertical slice with tests and verified CI. If a requirement is blocked by external hardware, record the blocker and move to another independent software task. Reconcile the canonical TODO only with completed acceptance evidence.

**Handoff scope:** documentation only. This file does **not** implement the proposed durability barrier, alter TODO checkboxes, qualify power loss or certify a release.
