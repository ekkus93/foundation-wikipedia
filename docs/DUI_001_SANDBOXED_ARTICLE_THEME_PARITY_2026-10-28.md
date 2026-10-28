# DUI-001 / STATE-002 — Sandboxed article theme parity

Date: 2026-10-28. Scope: desktop online article text-preview iframe.

## Implementation

The parent Tauri/React shell already offered persisted light, dark and system theme preferences, but the isolated `srcDoc` article preview always used dark text on a white background. The reader now passes the selected whitelisted theme through `App` and `OnlineReader` to `isolatedReaderDocument`. Explicit light/dark choices use fixed foreground/background CSS literals; the system choice uses a fixed `prefers-color-scheme: dark` media query. Invalid values default to the light palette. No article-controlled CSS is interpolated, and the iframe retains `sandbox=""` plus the existing deny-by-default CSP, including disabled external images and network access.

## Verification

`desktop/scripts/readerLayout.test.mjs` verifies explicit light/dark CSS, system preference CSS, default-light behavior for injected theme strings, and retention of the CSP. Existing text-size/width and HTML-isolation tests remain.

Source SHA: `bc98ebae912762b01cd011d02a5671b85a52e395`.

Exact-head CI passed: [Rust workspace 40674405548](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40674405548), [Platform shells 40674405566](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40674405566), and [Desktop launch smoke 40674405448](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40674405448).

## Remaining

Theme changes still replace `srcDoc` and may reset iframe scroll. No physical browser/device visual comparison, high-fidelity image/math rendering, verified installed offline media or source-authenticated snapshot is demonstrated. **DUI-001 and STATE-002 remain unchecked.**
