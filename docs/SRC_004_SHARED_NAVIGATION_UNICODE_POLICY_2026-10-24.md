# SRC-004 / SEC-001 — shared navigation Unicode policy (2026-10-24)

## Implementation

Before this change, the online search and article parsers, title/redirect resolver, request builder and verified local title index independently rejected only selected bidi controls. That left inconsistent handling of Unicode noncharacters and invisible format/separator markers, including cases rejected by the revision-keyed temporary HTML cache. These are metadata/navigation integrity boundaries, not evidence of publisher authentication.

The modules now reuse `wiki-source::online_requests::safe_navigation_text` with their existing distinct byte limits. It rejects C0/C1 controls, Unicode noncharacters U+FDD0–U+FDEF and the U+*FFFE/U+*FFFF plane tails, U+200B, U+2028–U+2029, U+2060, U+206A–U+206F, U+FEFF and existing bidi directional controls. Persian ZWNJ (U+200C), Indic ZWJ (U+200D), Arabic/Hebrew text and ordinary multilingual titles remain accepted.

The policy is applied to request search/title inputs, parsed current-revision and rendered titles, parsed search hits, requested/canonical/redirect resolution labels, verified local catalog titles, redirect aliases and local search prefixes. Synthetic regressions exercise malformed titles in online requests, search JSON, revision metadata, title resolution and verified offline catalog/redirect construction.

## Exact-head qualification

- Initial source `011e9870848e544a92dcc64c70b4a70274b20dc2`: [Rust workspace 40131110463](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40131110463) failed rustfmt-only test formatting.
- Additional boundary tests `dc0013713b072f397ceef394da7500ecfa219b19`: [Rust workspace 40131437881](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40131437881) failed a rustfmt-only multiline assertion.
- Corrected exact code **`01ae1e21b2ccb8e69850d6f0a606bd9301e26789`**: [Rust workspace 40131484535](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40131484535) passed formatting, strict Clippy and workspace tests; [Platform shells 40131484474](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40131484474) passed.

## Remaining scope

This is a conservative navigation-label integrity safeguard; it does not establish language-specific MediaWiki title equivalence, upstream source authenticity/completeness, safe final rendering of all content, mandatory offline media availability, or physical desktop/Android acceptance. **SRC-004 and SEC-001 remain unchecked.**
