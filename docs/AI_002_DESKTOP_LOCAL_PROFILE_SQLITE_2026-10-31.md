# AI-002 — Durable non-secret local model preference (desktop)

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified implementation SHA:** `bcb315ac85e142d92594514406e776b95c701632`  
**Scope:** One saved local profile; AI-002 and AI-004 remain incomplete.

## Implementation

Desktop Settings now offers an explicit **Save local profile** action after a successful discovery and model selection. Native Tauri `read_local_ai_profile` and `save_local_ai_profile` use the existing private SQLite user-state store, with a single `ai.local_profile.v1` key. Its serialized value has exactly three allowed non-secret fields: provider protocol, pinned local HTTP endpoint, and exact validated model ID. No API key, password or article/chat content is stored.

Native validation rejects public/DNS destinations, credential-bearing URLs, HTTPS that cannot yet be authenticated, malformed provider identifiers, anti-spoofed model IDs and saved JSON with extra fields. Model-discovery networking remains explicitly user-initiated. Restoring saved Settings does not contact a provider; the saved model ID is an **unverified hint**. The UI only selects the remembered model again if the chosen server's freshly discovered inventory advertises an exact match. If absent, it displays a warning and requires a new model choice.

Changes are limited to `desktop/src-tauri/src/local_ai.rs`, Tauri IPC registration/dependency, the AI Settings React component and its strict saved-profile validator/tests. Offline mode continues to block local-network discovery.

## Verification

- [Rust workspace 41256972267](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41256972267): passed exact source SHA, including workspace format, Clippy and tests.
- [Platform shells 41256972290](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41256972290): passed React frontend, native Tauri adapter tests and Android shell.
- [Desktop launch smoke 41256972311](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41256972311): running when this note was written; not claimed passed.
- Native regression exercises in-memory SQLite saved profile roundtrip, missing preference, public/credentialed/HTTPS endpoints, model Unicode control and corrupt saved JSON/unknown field rejection.
- Node test validates strict saved-profile shape and non-secret content; previous model-inventory tests remain.

## Remaining acceptance

- Multiple saved profiles, explicitly selected default and proper provider enable/disable/consent UI.
- Real local Ollama/llama-server LAN integration and authenticated HTTPS.
- Safe reader-to-chat inference, article-scoped RAG retrieval and verified citation streams.
- User-state/profile cross-process concurrency and desktop/Android real-device/restart acceptance.

**Do not check off AI-002 or AI-004** based solely on this partial preference workflow.
