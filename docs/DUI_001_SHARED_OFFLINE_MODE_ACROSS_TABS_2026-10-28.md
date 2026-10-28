# DUI-001 / UX-004 — Shared desktop offline mode across article tabs

Date: 2026-10-28. Scope: desktop online text-preview shell, not offline-ready article installation.

## Problem and change

Previously each mounted `OnlineReader` had its own `offline` React state, initialized to false. Switching to Work offline in one tab left other already-mounted tabs online; creating another tab silently reset the switch to online. That violated the expected application-wide intent of the offline control and allowed a hidden tab to initiate requests while another tab displayed Work offline.

The application now owns a single `ReaderOfflineMode` store, passed to every tab and subscribed to using React `useSyncExternalStore`. The store changes synchronously and notifies all mounted tabs. Each tab invalidates its request epoch, releases busy state and clears transient search hits when the shared switch becomes offline. Search, article-title/page and saved-bookmark handlers check the authoritative store before initiating requests and again before accepting late responses. The switch is inherited by newly created tabs and remains active through ordinary tab changes.

Already rendered, sandboxed exact-revision text previews remain accessible but are explicitly **not** installed offline-complete articles. No additional iframe privileges or network endpoints were introduced. The label says **no new network requests**, because already-running native HTTP is not actively canceled by this change.

## Verification

Implementation SHA: `758ce1d96bc24d81c746701376a19fd18ed1933e`.

- [Rust workspace CI 40684648182](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40684648182): success.
- [Platform shells CI 40684648399](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40684648399): success, including desktop TypeScript and frontend tests.
- [Desktop launch smoke CI 40684648099](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40684648099): success.
- `desktop/scripts/readerOfflineMode.test.mjs` checks multi-tab propagation, newly subscribed readers, repeated transitions, unsubscribe behavior, and cancellation epochs for visible/hidden readers.

## Remaining qualification

This is a shared UI request gate, **not** a proof that an already-running native request is interrupted at the socket. It does not integrate the verified installed snapshot/catalog/media store, enable real offline article/search navigation, prove complete required visuals, persist offline preference across application restart, or satisfy physical desktop/Android acceptance. **DUI-001, UX-004 and SRC-004 remain unchecked.**
