# TRUST-003 / PACK-001 — Canonical Unicode filesystem paths (2026-10-29)

## Scope and rationale

The provisional unsigned `.wpack` manifest previously rejected case-equivalent object paths but accepted Unicode names with canonically equivalent spellings. For example, `media/café.svg` and `media/cafe\u{301}.svg` can denote the same filesystem entry on normalization-sensitive platforms. A manifest containing only the decomposed form could also be installed under a different on-disk spelling, undermining portable object-path identity.

## Implementation

- `wiki-pack-format::manifest::unsafe_component` now rejects any component whose Unicode NFC normalization differs from its literal spelling. The same validator covers every object path segment and filesystem-facing pack metadata identifiers (pack ID, snapshot and origin identifier).
- `unicode-normalization = 0.1.25` is an explicit dependency; that exact crate version was already present in the workspace lockfile, which now records the direct dependency.
- Synthetic tests reject decomposed filenames, directories and identifiers; accepted examples include composed `café` and Japanese path components. This does not silently rewrite manifest identity bytes.

## Verification

**Exact code SHA:** `9aca7e334548b5f70590e0eada8c4c49259afaea`

- [Rust workspace CI — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40904996432): formatting, Clippy and Rust tests, including the new manifest regressions.
- [Platform shells CI — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40904996380): desktop/frontend and platform qualification.
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40904996427): cross-compiled shared Rust surface.

No macOS HFS+/APFS or Windows filesystem acceptance was executed. NFC is necessary for canonical equivalence but not a complete locale-specific case-folding or Windows namespace policy. The format is still unsigned, and successful integrity checks do not authenticate a publisher or establish offline media completeness. **PACK-001 and TRUST-003 remain unchecked.**
