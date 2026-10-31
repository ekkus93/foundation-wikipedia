# AI-002 — Explicit discovered local provider/model selection

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified implementation SHA:** `7b3fa44a006b2b619fc6350407c5dc3b966fcc2d`  
**Scope:** Shared Rust API only. AI-002 remains incomplete.

## Problem addressed

The low-level Ollama and OpenAI-compatible adapters could discover and stream with a caller-supplied model, but they lacked a shared, displayable selection object tying the exact advertised model to a pinned explicit endpoint. This made desktop/Android profile and model selection harder to implement consistently and risked app-side model substitution.

## Implementation

- Added `crates/wiki-ai/src/local_selection.rs`, exported from `wiki-ai`.
- `discover_local_models(LocalProtocol::{Ollama,OpenAiCompatible}, endpoint_url)` validates a user-supplied exact localhost/private IP endpoint, discovers its bounded model inventory and returns a `LocalModelInventory` with provider kind, locality and the displayed destination.
- `LocalModelInventory::select` accepts only exact advertised model identifiers and produces a `SelectedLocalModel` carrying the same protocol and destination. Unknown/case-changed IDs fail.
- `SelectedLocalModel::stream` builds the request for the selected model, uses the existing `stream_selected` policy/cancellation gate and the existing pinned-socket transport. `OutboundPolicy::OnDeviceOnly` rejects both localhost and LAN before sending any conversation text.
- The module never persists keys, guesses a hostname via DNS, follows redirects, retries via a cloud service or claims a LAN-hosted model is running on the device. HTTPS remains unsupported until authenticated TLS is implemented; never downgrade it silently.
- No new dependencies, no HTTP server and no platform-specific dependencies were added.

## Regressions and CI

- Real loopback TCP fixture: Ollama native `/api/tags` discovery, case-sensitive exact selection, streamed OpenAI-compatible chat completion to the same host, and the serialized selected model ID.
- OpenAI-compatible `/v1/models` fixture: model metadata, exact ID validation, blocked cloud-named fallback, and denial of network output under `OnDeviceOnly`.
- Invalid public/DNS endpoints and unsupported HTTPS fail without falling back to any other provider.
- First source SHA `90714aabefb5b54093dbbb54689232a684e08443` failed rustfmt: [Rust workspace 41246228359](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41246228359).
- Corrected source SHA **`7b3fa44a006b2b619fc6350407c5dc3b966fcc2d`**: [Rust workspace 41246335172](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41246335172) **passed**; [Android Rust ABI 41246335161](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41246335161) **passed**; [Platform shells 41246335195](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41246335195) was running when this report was prepared.

## Not yet qualified

- End-user Settings/profile UI for endpoint and model selection, including clear network/locality indicators.
- Live Ollama and llama-server tests over LAN; server model changes after discovery are not cryptographically authenticated.
- Authenticated HTTPS, credential management for hosted models and explicit consent/fallback controls.
- Article-grounded retrieval, verified citation rendering, Android device acceptance and offline inference.
- Localhost/third-party services are *not* equivalent to an embedded offline on-device inference engine.

The canonical checklist must keep **AI-002 unchecked** until its full acceptance criteria are met.
