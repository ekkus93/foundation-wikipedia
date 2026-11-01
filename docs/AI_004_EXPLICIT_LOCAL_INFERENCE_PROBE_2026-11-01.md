# AI-004 — Explicit local model inference connection test

**Date:** 2026-11-01  
**Status:** Partial AI-002/AI-004; neither parent is complete.  
**Verified implementation SHA:** `00036900d8bac4f6cfe8d9861e965362efb0bfc2`

## Implementation

Desktop Settings → AI Assistant now offers **Test selected model** after explicit model discovery and selection. The action is disabled while offline, during other discovery/save/test operations, and when the protocol, endpoint, locality or model no longer matches the current discovered inventory. Changing the endpoint/model or enabling offline mode invalidates any pending UI result. Merely opening Settings, loading a saved profile, switching profiles, or saving a profile does not initiate inference.

The native `test_local_ai_model` command independently parses the protocol and validates the pinned localhost/private-LAN HTTP endpoint, rediscovers the server's model inventory and refuses to POST for an unadvertised model. It then sends **only** the fixed string `Reply OK.` as a user-role diagnostic, with `max_output_tokens: 8`, through the shared provider-independent streaming contract and explicit `AllowLocalEndpoints` policy. A successful stream must complete with nonempty assistant text. Native Rust discards generated content; IPC returns only success or a generic failure. No article text, user questions, bookmarks, history, API keys, cloud provider or fallback is involved.

The shared HTTP transport already enforces a 120-second maximum streaming duration, bounded response size, IP-pinned sockets, no DNS, no redirects and no TLS downgrade. Plain HTTP LAN testing is **not** suitable for sensitive prompts; this probe deliberately contains no sensitive prompt.

## Automated verification

- [Rust workspace — 41293417368](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41293417368) — passed.
- [Platform shells — 41293417400](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41293417400) — passed, including native Tauri tests and desktop TypeScript tests.
- [Desktop launch smoke — 41293417376](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41293417376) — passed.

New native TCP-fixture tests verify model discovery followed by an explicit inference POST containing only the fixed prompt and selected model; a model absent from discovery causes no POST; public or credential-bearing destinations and oversized IDs are refused. A Node regression tests the UI's exact inventory binding, offline denial, pending-operation denial and endpoint/protocol/model mismatch.

## Remaining qualification

This is a synthetic localhost inference test, not evidence of real Ollama or llama-server compatibility on the owner's LAN or a grounded answer from an actual Wikipedia article. There is no article passage-consent flow, chat streaming UI, verified citation integration, cloud provider secret storage or real-device acceptance. Switching offline or editing the selection discards late UI results but **does not yet cancel an already-started native HTTP probe**; active transport cancellation should be implemented before general chat inference. AI-002 and AI-004 remain unchecked.
