# Desktop preview navigation: partial qualification

At commit `ae18c3cd8e447c4f93f8cc170ec47e168331c9f0`, the desktop preview gained a bounded list of up to 80 same-edition, deduplicated mainspace article links outside the sandboxed iframe. The URL validator rejects cross-origin and cross-edition links, unsafe schemes, namespaces, queries, malformed encodings and path traversal. Node regression tests exercise accepted and rejected URLs. Embedded iframe navigation remains disabled.

Exact-head qualification passed: Rust workspace run 38499299523, Platform shells run 38499299459 (including frontend typecheck and Node tests), and Desktop launch smoke run 38499299421. The subsequent documentation-only head `5d2992d556bb899f53a4f2aaeb31e6d61cfdeab9` passed Rust workspace 38499593695 and Platform shells 38499593839.

This is not a completed SRC-004 or DUI-001. Outstanding: live Wikimedia navigation interoperability, browser-level click-through tests, inline article links, full history, required images/math/attribution, installed offline packs and real Android acceptance. Keep those canonical TODO items unchecked.

## Bounded back navigation (partial DUI-003)

Commit `6d9ead915a806d36aea110c1edd01cd0a27373b5` adds a bounded 50-entry article back stack, preserving the exact prior page ID, revision ID and HTML snapshot when following a validated article link. Search, edition changes and offline transitions clear the back stack rather than presenting stale online content. Deterministic Node tests cover exact revision restoration and the history bound.

Exact-head CI passed: Rust workspace run 38500054328, Platform shells run 38500054353 (including frontend typecheck/tests), and Desktop launch smoke run 38500054325. No automated browser interaction, scroll-position restoration, forward navigation or physical-device acceptance is claimed. DUI-003 remains unchecked.
