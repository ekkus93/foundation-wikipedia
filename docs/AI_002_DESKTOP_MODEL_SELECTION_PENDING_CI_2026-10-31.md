# AI-002 — Desktop local model selection (offline work; awaiting Ralph Bridge)

**Date:** 2026-10-31
**Based on uploaded repository snapshot:** `1e41c6cd9f2c2186ee5d4e8c606ee79778262afd`
**State:** Local patch, **not committed/pushed to GitHub**, **not CI-qualified**. Do not mark AI-002 complete.

## Implementation

- `desktop/src/AiAssistantSettings.tsx` adds a dedicated Settings → AI Assistant screen to select Ollama or OpenAI-compatible/llama-server, supply an explicit endpoint, discover models on demand, and choose one exact server-advertised model. The UI discloses localhost vs LAN/network dependence and intentionally does not imply local servers are embedded/offline inference.
- `desktop/src/aiLocalModels.ts` validates native inventory responses against the exact requested endpoint/protocol, bounded model IDs, Unicode anti-spoof controls, context limits, duplicate model IDs, and locality claims. The renderer never fetches URLs directly.
- `desktop/src-tauri/src/local_ai.rs` adds an async, nonblocking `discover_local_ai_models` native command backed exclusively by `wiki-ai::local_selection::discover_local_models`. It uses the shared pinned local destination, declines unsupported HTTPS and public/DNS destinations, sanitizes user-facing errors, and cannot silently fall back to cloud inference.
- The command is registered in `desktop/src-tauri/src/lib.rs`; `desktop/src-tauri/Cargo.toml` adds `wiki-ai` and `serde`; `desktop/src/App.tsx` mounts the section; `desktop/src/styles.css` adds usable light/dark-compatible styling.
- Work-offline transitions invalidate outstanding frontend discovery results. Editing the endpoint or provider also invalidates previous results and selection, guarding against stale model/destination UI.
- No credentials are collected or stored. Selection is intentionally **session-only** until profile persistence, consent and chat orchestration are implemented.

## Local checks performed

- `node --experimental-strip-types desktop/scripts/check-reader-actions.mjs`: **passed**.
- `node --experimental-strip-types --test desktop/scripts/*.test.mjs`: **64/64 passed**, including three new inventory tests.
- `tsc --noEmit --strict --target es2022 --module esnext --lib es2022,dom desktop/src/aiLocalModels.ts`: **passed**.
- TypeScript parser/transpile diagnostics for `AiAssistantSettings.tsx`, `App.tsx`, `aiLocalModels.ts`: **zero**.
- Full frontend `npm install` attempt **timed out**; there is no installed `node_modules` in this sandbox, so the complete React build/typecheck was not run.
- Cargo/rustc are **not installed** in this sandbox. The desktop Tauri Rust adapter has **not** been compiled or Clippy-tested here.
- Ralph Bridge health and remote repo-state calls failed with **tunnel-client not seen for 300 seconds**. No GitHub source or CI operations were performed using a substitute connector.

## Required next steps

1. Restore Ralph Bridge and fetch the current exact `master` SHA and latest full TODO/spec. Compare with snapshot `1e41c6cd9f2c2186ee5d4e8c606ee79778262afd` and reconcile any concurrent changes.
2. Apply/rebase the accompanying `.patch` on current `master` without creating a task branch or PR.
3. Run frontend `npm install`, `npm run build`, native `cargo fmt --check`, `cargo test`, strict Clippy, and an actual desktop launch/smoke test. Fix any compiler/linter failures.
4. Commit through Ralph Bridge with exact-head CAS and inspect resulting exact-commit GitHub Actions including desktop shell/Tauri and Android ABI as applicable. Update the canonical `docs/WIKIPEDIA_AI_READER_TODO.md` with *qualified* evidence only after passing checks.
5. Complete persistent provider profiles/secure credentials, real Ollama and llama-server LAN acceptance, authenticated HTTPS, on-device vs LAN indicators in chat, and article-grounded citation verification. **AI-002 remains unchecked.**
