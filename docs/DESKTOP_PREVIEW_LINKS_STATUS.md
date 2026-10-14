# Desktop preview navigation: partial qualification

At commit `ae18c3cd8e447c4f93f8cc170ec47e168331c9f0`, the desktop preview gained a bounded list of up to 80 same-edition, deduplicated mainspace article links outside the sandboxed iframe. The URL validator rejects cross-origin and cross-edition links, unsafe schemes, namespaces, queries, malformed encodings and path traversal. Node regression tests exercise accepted and rejected URLs. Embedded iframe navigation remains disabled.

Exact-head qualification passed: Rust workspace run 38499299523, Platform shells run 38499299459 (including frontend typecheck and Node tests), and Desktop launch smoke run 38499299421. The subsequent documentation-only head `5d2992d556bb899f53a4f2aaeb31e6d61cfdeab9` passed Rust workspace 38499593695 and Platform shells 38499593839.

This is not a completed SRC-004 or DUI-001. Outstanding: live Wikimedia navigation interoperability, browser-level click-through tests, inline article links, full history, required images/math/attribution, installed offline packs and real Android acceptance. Keep those canonical TODO items unchecked.
