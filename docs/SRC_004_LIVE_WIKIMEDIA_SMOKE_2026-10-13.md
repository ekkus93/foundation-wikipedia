# SRC-004: opt-in live Wikimedia HTTPS smoke acceptance

This test is deliberately **not** evidence of completed SRC-004, offline packs, Android network transport, or production reader UI. It exercises the existing host-only `CurlTransport` against the actual Wikimedia Action API and the revision-pinned fetch pipeline.

## Run

From the repository root, on a host with a trusted system CA store, `curl`, Rust 1.85+, and outbound HTTPS:

```sh
cargo test -p wiki-source --test live_wikimedia --locked -- --ignored --nocapture
```

Ordinary `cargo test --workspace --locked` compiles the tests but skips external network requests. CI can run this as an explicitly authorized network qualification job rather than making all PR/push gates depend on a third-party service.

The smoke checks that real enwiki search returns valid mainspace navigation identities; title lookup resolves to a stable page ID; current-revision metadata is followed by an exact-oldid rendered response; and a separate page-ID request resolves to the same page. A Wikipedia edit between the two requests can legitimately change the revision ID, so the test does not incorrectly require their revision IDs to match.

## Scope and limitations

- A passing live smoke demonstrates host TLS/Action API interoperability at the tested time and environment, not the authenticity of independently downloaded media or completeness of offline visuals.
- The transport uses a system `curl` executable with default certificate verification, fixed timeouts, no redirect following, canonical read-only API URLs and response byte caps. Android requires its own reviewed native HTTPS adapter.
- Live upstream availability, throttling, regional connectivity and changing API behavior can cause an opt-in smoke failure. Preserve the exact commit SHA, command, UTC date, runner OS and output in qualification evidence before drawing conclusions.
- SRC-004 stays unchecked until actual reader integration, required media and offline/device acceptance are demonstrated.
