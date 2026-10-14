# Desktop preview navigation: partial qualification

At commit `ae18c3cd8e447c4f93f8cc170ec47e168331c9f0`, the desktop preview gained a bounded list of up to 80 same-edition, deduplicated mainspace article links outside the sandboxed iframe. The URL validator rejects cross-origin and cross-edition links, unsafe schemes, namespaces, queries, malformed encodings and path traversal. Node regression tests exercise accepted and rejected URLs. Embedded iframe navigation remains disabled.

Exact-head qualification passed: Rust workspace run 38499299523, Platform shells run 38499299459 (including frontend typecheck and Node tests), and Desktop launch smoke run 38499299421. The subsequent documentation-only head `5d2992d556bb899f53a4f2aaeb31e6d61cfdeab9` passed Rust workspace 38499593695 and Platform shells 38499593839.

This is not a completed SRC-004 or DUI-001. Outstanding: live Wikimedia navigation interoperability, browser-level click-through tests, inline article links, full history, required images/math/attribution, installed offline packs and real Android acceptance. Keep those canonical TODO items unchecked.

## Bounded back navigation (partial DUI-003)

Commit `6d9ead915a806d36aea110c1edd01cd0a27373b5` adds a bounded 50-entry article back stack, preserving the exact prior page ID, revision ID and HTML snapshot when following a validated article link. Search, edition changes and offline transitions clear the back stack rather than presenting stale online content. Deterministic Node tests cover exact revision restoration and the history bound.

Exact-head CI passed: Rust workspace run 38500054328, Platform shells run 38500054353 (including frontend typecheck/tests), and Desktop launch smoke run 38500054325. No automated browser interaction, scroll-position restoration, forward navigation or physical-device acceptance is claimed. DUI-003 remains unchecked.

## Memory-bound history follow-up

Exact `14d014d06796dc244a0271794d640db9eb3346d0` caps the back stack at 12 snapshots and four million retained HTML characters, dropping older revisions first to prevent unbounded memory growth. Node regressions cover the count and aggregate HTML budget. Rust CI 38500430404, Platform CI 38500430510 and Desktop launch smoke 38500431250 all passed at that exact code SHA. Browser navigation, scroll restoration and physical-device qualification remain pending; DUI-003 stays unchecked.

## 2026-10-14 — repaired build, independent tabs, stable search identities

- Commit `f4b182aabf2130ea2c44a7af80937c57e5329e25` restores the missing JSX closing brace for the preview Forward control. Exact-head Rust/Platform/Desktop-smoke CI: 38523850438 / 38523850503 / 38523850450, all successful.
- Commit `d93b4e85ecd29426f2f94c190621137db72a588b` adds a bounded four-tab desktop reader. Individual readers stay mounted when switching tabs, so each retains its own article/revision and bounded Back/Forward history. Node regression tests cover addition, selection, close, monotonic IDs and limits. Exact-head Rust/Platform/Desktop-smoke CI: 38524631793 / 38524631841 / 38524631783, all successful.
- Commit `345fd5a38ceb912372c56f5db7d1894e1d018ad0` adds `fetch_page_id` to the shared Rust source module, `open_wikipedia_page` to Tauri, and routes selected search hits by stable page ID rather than mutable title. Same-edition article link routing still resolves titles. Deterministic Rust tests cover exact page/revision and offline no-network behavior.
- Formatting remediation `14a4d34e863355ea8b47f90cc4124bffa843d48b` passed exact-head Rust workspace 38525426087 and Platform shells 38525426114, including frontend tests. The preceding initial Rust run 38525332197 failed solely at rustfmt and is not counted as qualification.

**Not qualified as complete:** SRC-004 still needs live official-Wikimedia interoperability, cache/storage and freshness semantics; DUI-001 needs full inline rendering, required media and browser acceptance; DUI-003 still needs reliable in-iframe reading-position restoration and real click-through/browser validation. The canonical TODO parent tasks remain unchecked. Passing CI demonstrates these narrower deterministic implementations, not offline completeness, live service availability, publisher trust or Android acceptance.

## 2026-10-14 — article-aware tab state and bounded iframe position retention

- `55a0e9084babdaf7af7fa8d71f2bc6b428e6ea5b` makes each tab's label track the displayed canonical article title; pure Node tests cover rename, length caps, reset and unaffected neighboring tabs. Exact-head Rust 38526026858, Platform shells 38526026799 and Desktop smoke 38526026884 succeeded.
- `022dce57df9e4474528b85db78f4e3d92380a3b0` snapshots the **outer page** scroll offset separately per tab and restores it after switches and returning from Settings. Exact-head Rust 38526520258, Platform shells 38526520303 and Desktop smoke 38526520244 succeeded. A write-tool transport error was reconciled by verifying the actual remote master commit and diff, not by blindly repeating the mutation.
- `537eda38f474654ce927addfaea89eb950dff95c` retains only current plus the two nearest Back and two nearest Forward **sandboxed iframe DOM instances** in each mounted tab. Stable frame IDs survive navigation so a WebView may preserve in-iframe reading position without enabling scripts or same-origin privilege. Node regression verifies cache membership and stable revision/frame identity. Exact-head Rust 38527170778, Platform shells 38527170732, Desktop smoke 38527170708 succeeded.
- `9d13751ad5a44afb5c95ade9b77fbb56fc217a7e` gives the sandboxed article preview responsive full-width, viewport-height sizing. Exact-head Rust 38527777364, Platform shells 38527777376, Desktop smoke 38527777299 succeeded.
- `1ba8b9c0a39c119907e80303a912e277f41daea6` fixes duplicate form control IDs across simultaneously mounted tab panels using React `useId`. Exact-head Rust 38527962154, Platform shells 38527962164 and Desktop smoke 38527962193 succeeded.

**DUI-003 remains unchecked:** actual WebView scroll restoration under Back/Forward and hidden-iframe reactivation has not been demonstrated in an interaction test; older-than-cache visits intentionally re-create iframe DOM. Persisted reading history, full Wikipedia rendering and installed offline article state are also missing. **DUI-001 and SRC-004 remain unchecked.** These tests do not assert full online interoperability, offline media, physical Android behavior, or release readiness.
