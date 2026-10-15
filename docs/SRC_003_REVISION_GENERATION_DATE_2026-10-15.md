# SRC-003 — Diagnostic XML revision publication-date guard (2026-10-15)

**Status:** Partial source-validation improvement; `SRC-003` remains unchecked.

## Invariant

A diagnostic MediaWiki current-content XML member cannot credibly claim a revision timestamp later than its declared source-generation calendar day. The Python bzip2/XML decoder previously checked timestamp syntax and actual calendar validity but did not enforce publication-date consistency, although the Rust downstream diagnostic JSONL reader already rejected later-day revisions.

The decoder now fails closed when the date part of any page revision timestamp is later than the pinned generation date, including pages excluded by the selected namespace filter. It performs the check **before writing page IDs or content records**, retaining the existing pending-file cleanup/no-publication semantics.

Regression cases cover all three diagnostic export modes (`ids`, `revision-jsonl`, `revision-content-jsonl`), an excluded namespace carrying an invalid future revision, and a valid prior-day revision. Full source authenticity, upstream completeness, production Rust XML/bzip2 parsing, rendering and mandatory media remain unqualified.

## Qualification

Implementation and tests are committed together directly on `master`. Exact-head Rust workspace and Platform shells CI outcomes must be verified and recorded before claiming this slice qualified. No canonical TODO task should be marked complete solely because this validation exists.

## Exact-head qualification (completed)

- Initial implementation SHA `a354b18ff0dcd47ddd7cc912b95a1a6f8167fbed` was **not qualified**. [Rust CI 38724342118](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38724342118) failed fixture-provenance Python unittest discovery because the new test insertion accidentally nested an existing test method (an `IndentationError` at line 390). An incorrect byte-literal newline expectation was also found during inspection.
- Repair SHA **`f01c32a7a92d23d9b7aa18b43a3bb57d5949a46d`** fixes the indentation and expected newline without weakening the generation-date guard. [Rust workspace CI 38724445336](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38724445336) and [Platform shells CI 38724445305](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38724445305) both **passed** at this exact head, including deterministic Python tests.
- No branch or PR was created. Canonical TODO remained exactly 1,536 lines with original blob SHA `ec594ed49cf4fff5e1c7ab42fca927881be55df5`; the original SRC-003 census document remained intact.

This qualifies the described narrow source-consistency checks only. The broader `SRC-003` TODO remains unchecked.
