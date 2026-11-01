# AI-004 — Cancellable local model inventory GET for explicit diagnostics

**Date:** 2026-11-01  
**Scope:** Partial AI-002 / AI-004; parent tasks remain unchecked.  
**Corrected source SHA:** `979c34cdbb2d4989910f4e9445f9f27061f74f5b`

## Problem

The prior explicit local inference diagnostic could cancel the streaming HTTP POST, but a stalled model inventory GET during native re-discovery continued until its three-second bounded timeout. Disabling Work offline or changing the selected model withdrew permission, yet the already-started inventory read could still wait before noticing cancellation.

## Implementation

- Added cancellation-aware model discovery entry points for both native Ollama `/api/tags` and OpenAI-compatible `/v1/models` using the same IP-literal/localhost endpoint parser, pinned socket address, no DNS/proxy/redirect policy, bounded JSON response and model validation.
- The shared discovery GET checks the existing `CancellationToken` before opening a socket, after connection, before/after writing the fixed GET, during each bounded response read and before returning parsed inventory.
- The desktop's **Test selected model** now uses cancellable rediscovery. Cancelling an active diagnostic during a stalled GET returns a cancellation error rather than proceeding to the inference POST.
- Existing synchronous discovery functions and provider trait behavior remain available to callers that do not pass a token. No remote/cloud endpoint is introduced; no article text is sent by the diagnostic.

## Automated evidence

The first implementation commit `07cefff8bafd953424b3cb53a60603fac00b4bdf` failed only `cargo fmt --all -- --check` ([Rust workspace 41299179640](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299179640)). Formatting was corrected in `979c34cdbb2d4989910f4e9445f9f27061f74f5b`, which passed:

- [Rust workspace — 41299226142](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299226142)
- [Platform shells — 41299226171](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299226171)
- [Android Rust ABI qualification — 41299226126](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41299226126)

Synthetic regression opens a real loopback TCP listener, accepts the fixed inventory GET and deliberately stalls its response while another thread cancels the request; the client returns `ProviderError::Cancelled`. A pre-cancelled token also refuses to open a socket. Existing synthetic probe tests cover no inference POST after unknown-model discovery and the fixed diagnostic prompt.

## Remaining boundaries

TCP `connect_timeout` and the small GET `write_all` are still bounded blocking operations rather than fully interruptible I/O; cancellation is observed at the next checkpoint, with 250 ms socket read polling during stalled reads. The ordinary user-initiated **Discover models** operation still lacks a native cancellation lease and may finish its bounded GET after the UI invalidates the response. This is synthetic localhost verification, not live Ollama/llama-server LAN acceptance or an interactive device timing measurement. It does not implement article-grounded chat, citation verification, consent to transmit passages, hosted-provider credentials or physical-device acceptance. **AI-002, AI-004 and AI-005 remain unchecked.**
