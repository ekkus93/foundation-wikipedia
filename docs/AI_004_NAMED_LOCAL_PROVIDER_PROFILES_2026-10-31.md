# AI-004 — Named local provider profiles (desktop, partial)

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source commit:** `20869f375162dedefabb6887e831b9f22b475204`  
**Status:** Scoped implementation complete; AI-002 and AI-004 remain unchecked.

## Functionality

- Desktop AI Assistant Settings now lists and manages **up to eight** user-named local Ollama/OpenAI-compatible provider profiles, with explicit create/update, active-profile selection and deletion.
- New Rust `desktop/src-tauri/src/local_ai_profiles.rs` uses the existing private SQLite `UserState` setting `ai.local_profiles.v2` with active name and bounded collection. Only non-secret fields are stored: profile name, protocol, localhost/private-LAN HTTP endpoint and model ID.
- The v1 single saved profile remains **read-only compatible** as the `Default` profile when no v2 state exists. The first explicit save/activate/delete writes v2 and retires the v1 row; an empty v2 record prevents resurrecting old settings.
- Persistent profiles are **hints, not connection authorization**: opening Settings, switching profiles or restarting does not query Ollama or call inference. A remembered model is reselected only after the user explicitly discovers that exact model at the selected endpoint.
- This does not create local/hosted API key storage, schedule model downloads or introduce cloud fallback.

## Validation

- Name rules: 1–40 ASCII letters/digits/spaces/dot/underscore/hyphen, beginning with alphanumeric; case-insensitive duplicate names are rejected.
- Each endpoint must pass the shared Rust pinned private-address parser and be `http://` only; credential-bearing and public/DNS targets fail closed. Models reuse the shared anti-spoofed outbound model validation.
- Inventory is limited to eight profiles and the serialized SQLite value to 4096 bytes. A saved collection must have exactly one valid active name when nonempty, or none when empty.
- Strict native serde and TypeScript response parsing rejects missing fields, extra/secret-looking fields, malformed profiles, duplicate names and invalid active targets.
- Read/modify/write uses the existing SQLite setting API. **Cross-process updates are currently last-writer-wins**; atomic compare-and-swap or revision fences and live two-window acceptance are still required.

## Exact-source CI

- [Rust workspace 41265251443](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41265251443) — **passed**.
- [Platform shells 41265251316](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41265251316) — **passed**, including frontend, Android shell and native Tauri tests.
- [Desktop launch smoke 41265251289](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41265251289) — **passed**.
- Native in-memory SQLite tests cover non-mutating legacy read, first-write upgrade, round trip, eight-profile cap, name collisions, invalid/public/credentialed endpoints, malformed model IDs, selection/removal and no-resurrection after deletion.
- Node tests cover bounded profile collections and strict active/name/model/endpoint response shapes.

## Work still necessary

- Multiple Windows/processes editing profiles concurrently need durable conflict handling, rather than a last-writer-wins overwrite.
- Real LAN Ollama and llama-server acceptance, authenticated TLS, model connection diagnostics and on-device versus LAN indicators need production validation.
- Hosted providers and OS-secure credentials, passage-sharing consent, policy-managed cloud fallback, article-grounded chat/citations and real desktop/Android-device acceptance remain unimplemented.
- The canonical top-level task count does not change: this is a **partial** AI-004 implementation.
