# DUI-003 — Edition-bound desktop navigation and bookmark provenance

Date: 2026-10-28. Scope: desktop online preview, saved-bookmark navigation and local history metadata only. This is not an offline-ready article implementation.

## Problem and change

The desktop reader previously stored a page ID, revision, title and HTML in each navigation snapshot but looked up its Wikimedia project from the mutable edition selector. During a cross-edition saved-bookmark transition, the selector and mounted article are independent React state updates. Using the selector as provenance could attribute a retained revision, history visit, bookmark FAB action or exact-revision source URL to the wrong edition. Numerically identical page/revision IDs across editions must never be treated as the same article.

`ArticlePreview` now carries its own immutable `project`. The reader uses that snapshot identity for native opt-in visit recording, the parent bookmark context, exact Wikimedia revision source URLs, and article-link navigation. Each request captures the requested project before its asynchronous response is accepted.

`openBookmarkedReaderArticle` now compares the actual mounted snapshot's project against the reopened article's project. Same-edition bookmark navigation retains Back history; cross-edition reopening resets it. The decision no longer trusts a potentially stale UI edition selector. The existing request epoch, stable page-ID open, sanitized iframe CSP and no-network offline behavior remain in force.

## Regression and CI

Source commit: `50959c61d135a84bd5605924fb79e0ff7f6b96e1` (three changed files: `desktop/src/OnlineReader.tsx`, `desktop/src/readerHistory.ts`, `desktop/scripts/readerHistory.test.mjs`).

- `readerHistory.test.mjs` exercises cross-edition saved navigation with the same numeric page and revision IDs, absence of cross-edition Back history, and preservation of same-edition exact-revision Back.
- Exact-head [Rust workspace 40658621441](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40658621441): success.
- Exact-head [Platform shells 40658621520](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40658621520): success.
- Exact-head [Desktop launch smoke 40658621586](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40658621586): success, including the desktop frontend test/build gate.

## Remaining acceptance

This is a scoped navigation correctness and provenance fix, not completion of DUI-003 or UX-003. Production offline catalog and required media verification, offline article opening, accurate iframe scroll restoration through Settings/tab changes, user-state migration/backup, live Wikimedia interoperability, physical desktop testing and Android integration remain outstanding. In-memory text previews are never labeled offline-complete.
