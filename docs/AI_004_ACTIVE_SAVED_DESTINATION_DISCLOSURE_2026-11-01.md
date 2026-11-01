# AI-004 — Distinguish persisted active AI destination from the editor draft

**Date:** 2026-11-01  
**Status:** Partial AI-004; parent remains unchecked.  
**Implementation:** `07010b26dea36ca9969631a86939c3d4f39d1c5c`

## Problem

The desktop AI Settings editor displayed a server URL and disclosed locality only after a successful model discovery. A user could edit the URL without changing the persisted active profile, or another process could change the active profile while the editor retained its unsaved draft. The interface did not explicitly show which destination was *actually saved as active*, separate from the editable draft.

## Implementation

- A pure, read-only `activeAiLocalProfileDestination` helper derives the exact active profile name, protocol, endpoint and conservative localhost/LAN classification from the validated saved collection. It returns no destination when there is no active profile, the active name is missing, or the endpoint is not an accepted local address.
- Desktop AI Settings displays the persisted active name, protocol, endpoint and locality independently of the editor's endpoint. The draft gets its own classification, including a clear invalid-endpoint notice.
- The disclosure explains that a saved profile does **not** authorize network requests or article-passage transmission, and that LAN HTTP is network traffic rather than on-device/offline inference.
- No new network operation, inference, credential storage, or cloud fallback was added.

## Verification

`desktop/scripts/aiLocalModels.test.mjs` covers active LAN and localhost switching, absent/missing active selection and invalid persisted endpoint. Exact-commit CI for the implementation passed:

- [Rust workspace — 41291442639](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41291442639)
- [Platform shells — 41291442704](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41291442704)
- [Desktop launch smoke — 41291442698](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41291442698)

## Remaining acceptance

This change improves Settings destination transparency only. AI-002/AI-004 still require live Ollama/llama-server LAN interoperability, explicit per-request passage disclosure and consent before inference, remote-provider credential protection and consent, grounded answer/citation integration, interactive cross-process desktop testing, and device acceptance. No top-level task is marked complete.
