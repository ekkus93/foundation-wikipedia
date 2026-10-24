# SRC-004 — verified local search suggestions on upstream outage

**Date:** 2026-10-24  
**Status:** Scoped CI-qualified source-adapter behavior, not complete SRC-004.

## Behavior

`wiki_source::online_search::fetch_search_with_local` retains the previous
online-first API behavior when an explicitly online search succeeds, but can
now surface *verified local* suggestions in an **error**, not in a fabricated
successful online search outcome:

```rust
SearchFetchError::UnavailableWithLocal {
    reason: Box<SearchFetchError<E>>,
    hits: Vec<LocalSearchHit>,
}
```

This variant is returned only when an online transport fails, or returns
oversized/non-UTF-8 bytes, **and** a caller-supplied `LocalTitleIndex`
returns verified local title/redirect suggestions for the query. The
`reason` retains the original transport/response failure; each hit retains
its actual local article origin, page ID and exact revision.

Successful online results take precedence. Malformed-but-readable or
identity-conflicting JSON remains a parse error, not a local success.
An absent index or zero matching local candidates returns the original
failure. A foreign-edition index is now rejected before the first network
request, including when the caller requested online mode.

No automatic UI fallback takes place: the platform adapter must label the
suggestions as local/offline and must not pass them to AI as evidence of a
fresh Wikimedia revision.

## Verification

- First code SHA `c274df2c91992e99e99e57f4976d80b39e4a8417`
  failed formatting: [Rust CI 40137499679](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40137499679).
- Follow-up `c140143ee50fe868b36aa72e14353555f256e03f`
  inadvertently dropped the tail of `online_search.rs`, causing a parser
  error: [Rust CI 40137567790](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40137567790).
- The complete file, all original tests, and the new behavior were
  restored in **`017c2a9812bb6fe9b0e31ce97ba1c5bed276f8be`**.
  [Rust workspace 40137738835](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40137738835)
  and [Platform shells 40137738907](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40137738907)
  both **passed** at this exact SHA.

The synthetic test covers offline index use without network, caller-labeled
upstream transport outage with local canonical title or redirect, non-current
local provenance and revision, missing local matches, absent index, successful
online precedence, parse failures that remain failures, and foreign-project
index isolation before HTTP.

## Limitations

The tests use an injected fake transport and an independently caller-verified
local index; they do not authenticate or benchmark Wikimedia production APIs.
The contract assumes the local index was built only from authenticated and
media-complete snapshots. Actual desktop/Android UI display, complete pack
install, offline required media, revision citation handling, network-failure
UX and physical-device testing remain outstanding. **SRC-004 remains unchecked.**
