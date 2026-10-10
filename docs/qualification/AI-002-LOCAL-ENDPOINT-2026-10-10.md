# AI-002 — local LLM endpoint classification (partial)

- Implementation: `crates/wiki-ai/src/local_endpoint.rs`
- Exact implementation SHA: `4af58435214a10705b4426ad244fd0a27001740c`
- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38076609892 — success
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38076609897 — success
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38076609901 — success
- Scope: reject public IPs, arbitrary DNS names, metadata/link-local addresses, credentials, unsafe paths and malformed ports. Distinguish explicit loopback and private LAN endpoints without cloud fallback.
- Tests: `cargo test --workspace --locked` in Rust workspace CI, including negative endpoint-classification cases.
- Outstanding: actual OpenAI-compatible/Ollama/llama-server HTTP adapters, model discovery, streaming/cancellation, destination pinning and real local-server acceptance.
- **Status: partial; AI-002 remains unchecked in the canonical TODO.** This evidence file is supplemental, not a competing checklist.
