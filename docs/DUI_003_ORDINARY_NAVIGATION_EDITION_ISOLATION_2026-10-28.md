# DUI-003 — Ordinary article navigation is edition-bound

Date: 2026-10-28. Scope: desktop in-memory online preview history only.

## Invariant

The previous desktop fix applied cross-edition isolation specifically when reopening bookmarks. Ordinary title, page-ID and extracted-link navigation still called `openReaderArticle` directly, so an asynchronous response with an unexpected Wikimedia edition could retain Back history from another wiki. Numeric page and revision IDs are scoped to their project; they are not global identities.

`openReaderArticle` now checks the immutable `project` of the current and incoming article snapshots. It discards prior Back/Forward state on a cross-edition transition while preserving exact-revision Back within an edition. `openBookmarkedReaderArticle` delegates to this same invariant, so bookmark and ordinary navigation cannot diverge.

## Regression and limits

`desktop/scripts/readerHistory.test.mjs` now tests same-edition Back and a cross-edition ordinary open with identical numeric page/revision IDs, including clearing both history stacks. Existing bookmark and retained offline-preview tests remain.

Source commit: `dce7cf76696dc419bdb73f08c341991386e8d924`. Exact-source CI passed: [Rust workspace 40666889203](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40666889203), [Platform shells 40666889062](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40666889062), and [Desktop launch smoke 40666889009](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40666889009). Documentation-fix head `d743f8491c1074fbb9ee3f6dc12bce2d4c3f82c2` passed [Rust workspace 40667113624](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40667113624) and [Platform shells 40667113710](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40667113710).

This is a local navigation correctness change, not a claim that the online preview has complete offline media or that physical desktop/Android acceptance has passed. Iframe scroll restoration across layout changes, verified installed-pack navigation and real-device testing remain outstanding.
