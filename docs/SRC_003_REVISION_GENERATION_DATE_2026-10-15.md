# SRC-003 — Diagnostic XML revision publication-date guard (2026-10-15)

**Status:** Partial source-validation improvement; `SRC-003` remains unchecked.

## Invariant

A diagnostic MediaWiki current-content XML member cannot credibly claim a revision timestamp later than its declared source-generation calendar day. The Python bzip2/XML decoder previously checked timestamp syntax and actual calendar validity but did not enforce publication-date consistency, although the Rust downstream diagnostic JSONL reader already rejected later-day revisions.

The decoder now fails closed when the date part of any page revision timestamp is later than the pinned generation date, including pages excluded by the selected namespace filter. It performs the check **before writing page IDs or content records**, retaining the existing pending-file cleanup/no-publication semantics.

Regression cases cover all three diagnostic export modes (`ids`, `revision-jsonl`, `revision-content-jsonl`), an excluded namespace carrying an invalid future revision, and a valid prior-day revision. Full source authenticity, upstream completeness, production Rust XML/bzip2 parsing, rendering and mandatory media remain unqualified.

## Qualification

Implementation and tests are committed together directly on `master`. Exact-head Rust workspace and Platform shells CI outcomes must be verified and recorded before claiming this slice qualified. No canonical TODO task should be marked complete solely because this validation exists.
