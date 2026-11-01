# AI-002 — Desktop local provider and model selection

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `35427663ae650f67b9d36c15608cc78ccc3ca85d`  
**Scope:** Desktop AI Assistant settings discovery/selection integration, **not** complete AI-002 acceptance.

## Implementation

The existing provider-independent Rust API now has its first desktop-facing UI integration:
- Settings → AI Assistant lets the user select Ollama or OpenAI-compatible/llama-server, supply an explicit validated localhost/private-network URL, discover models on demand and select an exact advertised ID for the current session.
- `desktop/src-tauri/src/local_ai.rs` implements async `discover_local_ai_models`, moving bounded synchronous discovery into a Tauri blocking worker while using `wiki_ai::local_selection` and its pinned socket, nonredirecting, non-DNS, local-only transport.
- `desktop/src/aiLocalModels.ts` rejects native responses with mismatched endpoint/protocol, invalid locality, duplicate/malformed/spoofed model identifiers, invalid context windows or oversized inventories.
- React invalidates pending discovery and cleared selection when endpoint/protocol changes or the global Work offline switch is enabled. Network-facing discovery is explicitly invoked by the user; it does not run silently on render.
- The UI explains that a LAN-hosted model is not an embedded/offline on-device model. HTTPS and public/DNS hosts are explicitly unsupported by this local adapter. No credentials, profile persistence or cloud fallback are introduced.
- The earlier local patch was reviewed and corrected: actually mount the new UI when the AI Assistant Settings section is chosen, and place the Rust module declaration after the crate-level documentation comment.

## Exact-source CI evidence

All three workflows on `35427663ae650f67b9d36c15608cc78ccc3ca85d` passed:
- [Rust workspace 41255096352](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41255096352): formatting, strict Clippy, tests and repository gates.
- [Platform shells 41255096596](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41255096596): desktop frontend, native Tauri adapter and Android shell.
- [Desktop launch smoke 41255096408](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41255096408): desktop launch qualification.
- Separate local dependency-free Node suite: **64/64 passed**; standalone TypeScript model validator typecheck passed. Native Cargo and full React dependencies were not available locally; hosted exact-commit CI provides the required build checks.

## Scope limitations and follow-up

- Model selection is **session-only and does not invoke inference**; end-to-end article-grounded chat, citation validation and streaming UI are still outstanding.
- Need durable provider profiles and model selection persistence, consent/disclosure controls and secure storage for cloud credentials.
- Need actual Ollama and llama-server connections across LAN, authenticated HTTPS, full Android UI wiring and physical device/manual acceptance.
- This work does not establish signed pack trust, complete offline media or a downloaded on-device inference runtime.

**AI-002 remains unchecked** until its original full acceptance criteria are satisfied.
