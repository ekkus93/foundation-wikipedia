# SRC-004 — verified title navigation fallback during upstream outage

**Date:** 2026-10-24  
**Status:** Scoped improvement, not completion of SRC-004/desktop acceptance.

## Problem

Online opening by title first requests current upstream title-to-page-ID
resolution. Before a successful response, the source library previously returned
a transport error with no fallback, even when the application supplied a
verified offline-complete local title index containing that exact requested
title or a verified redirect alias.

## Implementation

`wiki_source::online_fetch::fetch_title_with_local_index` now retains the
index-provided local `ArticleVersion` on **transport/unreadable response**
failure during title resolution, including transport errors, excessive response
bytes and invalid UTF-8. This version remains labeled by its actual installed,
saved or temporary-cached origin and exact revision; it is **not** relabeled
`OnlineCurrent` and is not silently substituted as a successful fetch.

The index's previously existing validation contract independently requires
complete local content/media, a valid project/page/revision, and verified
redirect-to-canonical bindings. Without a matching index entry no fallback
is returned.

An upstream *response* containing incompatible title/page identity,
an ambiguous/malformed title-resolution result, or a subsequent page-identity
mismatch still follows existing fail-closed behavior; a locally cached alias
cannot override authoritative identity data.

## Tests and exact-head CI

The synthetic `title_resolution_network_failure_preserves_verified_local_fallback_only`
test covers canonical and verified redirect alias outage cases, missing local
title, missing index, and incompatible upstream identity.

- Implementation SHA:
  `e2db7cd2def8d4fe78d7f551bf22e85e9d2994eb`
- [Rust workspace #40137157469](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40137157469):
  **passed** (formatting, Clippy and workspace tests).
- [Platform shells #40137157476](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40137157476):
  **passed**.

## Outstanding work

This only surfaces a safe optional local fallback on an error; a platform
adapter must choose how to present it visibly and must never mislabel it
current. There is no evidence here of end-to-end live Wikimedia API behavior,
installed-pack media completeness, user-interface handling or physical
desktop/Android acceptance. **SRC-004 remains unchecked.**
