# SRC-004 — Search-only Action API metadata isolation

**Date:** 2026-10-24  
**Status:** Scoped code and CI qualification; SRC-004 remains incomplete.

## Behavior

`wiki_source::online_search::parse_search_results` consumes only navigation
identities from a bounded Wikimedia `action=query&list=search` response.
It now rejects unexpected query-level `redirects`, `normalized`,
`interwiki`, and `pages` fields, including empty arrays. These fields
belong to other query modules and could otherwise be silently ignored
while a caller treated the response as a valid search result. Search hits
remain navigation hints only; a caller must resolve and verify an exact
article revision separately before treating content as citation evidence.

The check runs before result projection and applies even when the requested
visible result limit is zero. Expected search-only `searchinfo` metadata
remains accepted. The existing strict duplicate-key, mainspace identity,
title collision and response-size checks remain in force.

## Evidence

- Exact implementation SHA:
  `e01ca8413777aab1f94b5a9bb0b58f8d16a95630`.
- [Rust workspace CI 40139309687](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40139309687):
  success, including formatting, strict Clippy and workspace tests.
- [Platform shells CI 40139309567](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40139309567):
  success.
- Regression `rejects_unrequested_search_query_alias_and_page_metadata`
  covers all four forbidden keys, including empty arrays, and a positive
  ordinary `searchinfo` case.

## Remaining acceptance gaps

These synthetic parser tests do not establish actual Wikimedia API
interoperability, upstream publisher authentication, offline media
completeness, integrated desktop/Android UI behavior, or real-device
acceptance. Do not mark SRC-004 or the end-to-end release complete on this
evidence alone.
