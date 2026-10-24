# PACK-002 — Selection-review warning and queue bounds

**Date:** 2026-10-23  
**Status:** Partial implementation; PACK-002 remains unchecked.

## Change

`preview_pinned_selection` now uses the same bounded deterministic warning
collector as `resolve_pinned`. Its additional diagnostics (suppressed explicit
includes, high-fanout categories, detected cycles and page-limit warnings)
cannot grow an unbounded retained warning set. The first 256 warnings in
lexicographic order are preserved; if additional warnings exist, an explicit
truncation notice is appended. A preexisting resolver truncation notice is
recognized and retained without duplication.

The review's breadth-first category queue now deduplicates roots and children
when enqueued, matching the resolver's first-discovered-depth behavior. Raw
child-mention counts are unchanged, and category cycle diagnostics remain
bounded and independent of duplicate queue entries.

## Regression and CI evidence

- 321 excluded explicit includes yield bounded review warnings while keeping
  the same two selected page IDs.
- 2,048 duplicate roots and 6,144 child mentions visit only two unique
  categories; a minimal equivalent graph retains the same selected IDs and
  capped warnings.
- A small suppressed include produces its diagnostic without truncation.
- Initial code `a5c1a60742075328a2b9f116d5df4cea16402660`
  failed [Rust formatting 40051330692](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40051330692).
- Formatting repair `42113c16c5e6c64607d39195760c7b4e71f2f07a`
  passed [Rust workspace 40051379546](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40051379546).
  [Platform shells 40051379552](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40051379552)
  was still in progress at documentation; check terminal status.

## Outstanding acceptance

This protects diagnostic preview resource use over caller-supplied data.
It does not establish publisher-authenticated category completeness,
production UI integration, mandatory offline visuals, a verified pack build,
or real-device acceptance. PACK-002 remains unchecked.
