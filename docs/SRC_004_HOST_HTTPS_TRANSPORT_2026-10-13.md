# SRC-004 — Host HTTPS transport qualification (partial)

**Date:** 2026-10-13  
**Code SHA:** `5b2929dccce50f7aa2c7dd9a4b01db2ca8888bf1`  
**Status:** Host transport + revision-cache integration compiled and unit-tested; full SRC-004 acceptance **not achieved**.

## Implemented

- `crates/wiki-source/src/online_curl.rs`: explicit host-only curl transport. It accepts only canonical official HTTPS Wikipedia Action API requests. Redirect following is disabled and non-200 HTTP statuses are rejected. Curl's user config is disabled, certificate verification is left enabled, connect/total deadlines are bounded, and a streaming byte ceiling protects against unknown or false Content-Length. An identifying User-Agent and JSON Accept header are supplied.
- `crates/wiki-ffi/src/online_reader.rs`: host-only entrypoints for page-ID and title navigation through the existing exact-revision fetch and temporary HTML cache. Offline mode makes no subprocess or network request; HTML-only cache entries are not considered complete offline articles.
- Deterministic tests use a fake curl executable and fake transport to check URL preflight, redirect status, body cap, and offline zero-I/O. No external Wikimedia requests are performed in the automated unit tests.

## Exact-head evidence

- Rust workspace: [38474148175](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38474148175) — passed on `5b2929dccce50f7aa2c7dd9a4b01db2ca8888bf1`.
- Platform shells: [38474148071](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38474148071) — passed on the same exact head.
- Android Rust ABI: [38473907387](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38473907387) — passed on earlier host-wrapper code head `369307999a8dbf2ca5aba60a30a9d4570f42f1cc`. This does **not** qualify Android HTTPS or a physical-device article reader.

## Trust and deployment boundaries

This adapter delegates TLS certificate verification to the installed system curl and CA configuration; it is not a self-contained Rust TLS stack. It must not be used to claim publisher signatures, article-media completeness or verified offline installation. The host must supply a sufficiently modern curl binary and trust store; the Android target intentionally excludes the adapter. The actual desktop Tauri command and Android networking stack remain unimplemented.

## Outstanding acceptance

Run real official Wikipedia HTTPS article/search/redirect/revision smoke tests; wire results to desktop and Android reader states; validate required images, math, references, styles, attribution and offline media completeness; qualify real desktop navigation, Android native transport, and airplane-mode behavior. Keep **SRC-004 unchecked** until these are evidenced.
