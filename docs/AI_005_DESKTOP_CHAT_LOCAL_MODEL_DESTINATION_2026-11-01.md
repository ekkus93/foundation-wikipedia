# AI-005 — Read-only desktop chat local-model destination disclosure

**Date:** 2026-11-01 (America/Los_Angeles)  
**Source SHA:** `9cea43c76f59a5a74775c1250d5bdf219b2b0fe3`  
**Status:** Partial integration; **AI-005 remains unchecked**.

## Scope and rationale

The desktop conversation-history panel previously did not disclose the saved local model destination. This is a prerequisite for an eventual consent preview, but it must not suggest that the saved profile itself authorizes an inference request or that online HTML previews are canonical RAG evidence.

The new `ReaderChatProviderStatus` reads the existing native `read_local_ai_profiles` command and uses the shared strict TypeScript parser. It displays the **persisted active** profile name, exact model ID, Ollama/OpenAI-compatible protocol, endpoint and localhost-versus-private-LAN classification. If the active profile is disabled or removed, the panel shows no active model rather than silently choosing another saved profile. A failed metadata read displays an error; no endpoint is contacted.

Metadata refresh runs on mount, same-process Tauri profile-change notifications, window focus/visibility and a five-second poll while visible to cover separate processes. Requests are coalesced and generation-fenced; stale responses and responses after unmount are ignored. All refreshes read private SQLite metadata only.

The chat panel remains **read-only**. The disclosure explicitly states that a saved model hint is neither a verified connection nor consent to transmit article passages. It cannot generate answers from an online HTML preview.

## Verification

- Extended `desktop/scripts/aiLocalModels.test.mjs` with active-versus-disabled selection, exact model/endpoint, LAN locality, explicit switching and invalid-profile rejection.
- Existing desktop frontend typecheck/lint and Node test suite run in Platform shells CI.
- [Rust workspace 41386164543](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41386164543), [Platform shells 41386164515](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41386164515) and [Desktop launch smoke 41386164538](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41386164538) were running when this report was written. The desktop-frontend job had passed.
- The CI result is not a substitute for interactive two-window acceptance.

## Remaining AI-005 work

The desktop still needs a publisher/source-qualified canonical article for the exact displayed revision, a pure evidence preview with exact passages, explicit per-request authorization, a selected-model inference call with cancellation, validated-answer persistence, and revision-bound clickable citations. The existing shared Rust use case is not yet wired to the desktop UI. Android chat, live local-model acceptance and physical-device qualification remain outstanding.
