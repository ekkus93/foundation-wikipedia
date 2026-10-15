# SRC-003/PACK-003 — bounded visual URL discovery (2026-10-15)

**Status:** Diagnostic candidate inventory only; not a complete or verified
pack media manifest. Both SRC-003 and PACK-003 remain unchecked.

The renderer comparison previously counted images but did not retain the
actual candidate resource URLs needed to investigate missing offline visuals.
A new deterministic HTMLParser-based inventory enumerates `img src`,
responsive `img srcset`, `picture/source srcset`, SVG image/use references,
stylesheet link URLs and video poster/preview images. It deliberately does
**not** classify streamed audio/video payload URLs as required offline media.

Candidates are sorted and deduplicated by kind and URL, capped at 10,000 per
article and 4,096 characters per URL, and only included in the retained
renderer **JSON artifact**, not stdout/stderr diagnostics. Invalid authority,
oversized URLs and resource budgets fail closed; unsupported data/javascript
schemes are counted but never treated as installable remote objects. Protocol-
relative candidates are normalized to explicit HTTPS for discovery only.
Tests cover alternate variants, inline SVG, custom preview posters,
duplicate URLs, untrusted schemes and bounds.

The scoped official-source renderer comparison now records `local` and
`live` candidate lists plus aggregate candidate counts. It does NOT fetch
bytes, follow redirects, validate host ownership/licensing/attribution,
parse CSS `url()` or dependent fonts, handle every legal `srcset` grammar,
sanitize SVG, or prove that all required visuals were discovered. Inline CSS
is explicitly counted as a remaining blind spot. Candidate URLs are
untrusted input; any later downloader must revalidate origin and request
destinations, hash bytes, obtain Commons metadata, and fail closed before
a pack may claim offline-ready.

**Qualification:** only claim this slice after exact-head Rust/Python,
Platform and renderer CI pass.
