# DUI-003 — Ordinary article navigation is edition-bound

Date: 2026-10-28. Scope: desktop in-memory online preview history only.

## Invariant

The previous desktop fix applied cross-edition isolation specifically when reopening bookmarks. Ordinary title, page-ID and extracted-link navigation still called `openReaderArticle` directly, so an asynchronous response with an unexpected Wikimedia edition could retain Back history from another wiki. Numeric page and revision IDs are scoped to their project; they are not global identities.

`openReaderArticle` now checks the immutable `project` of the current and incoming article snapshots. It discards prior Back/Forward state on a cross-edition transition while preserving exact-revision Back within an edition. `openBookmarkedReaderArticle` delegates to this same invariant, so bookmark and ordinary navigation cannot diverge.

## Regression and limits

`desktop/scripts/readerHistory.test.mjs` now tests same-edition Back and a cross-edition ordinary open with identical numeric page/revision IDs, including clearing both history stacks. Existing bookmark and retained offline-preview tests remain.

This is a local navigation correctness change, not a claim that the online preview has complete offline media or that physical desktop/Android acceptance has passed. Iframe scroll restoration across layout changes, verified installed-pack navigation and real-device testing remain outstanding.
