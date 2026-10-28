# MOD-001 / SEC-001 — Canonical typed navigation-title boundary (partial)

**Date:** 2026-10-28  
**Implementation SHA:** `a512468167c398fd728924716cf7d3c2eaa4208d`  
**Scope:** `crates/wiki-model/src/lib.rs` canonical Article/Redirect validation.

## Implemented

The canonical model now applies a shared navigation-identity policy to article titles, article aliases and redirect titles, including caller-constructed DTOs that bypass the Enterprise/public-dump decoders. It rejects title strings longer than 4,096 UTF-8 bytes, leading/trailing whitespace, C0/C1 controls, bidirectional formatting overrides/isolates, U+061C, U+200B, U+2060, U+FEFF and U+2028/U+2029. Valid multilingual labels and meaningful U+200C/U+200D joining controls remain accepted. Invalid canonical article titles return `ModelError::InvalidTitle`; aliases return `InvalidAlias` and redirects return `InvalidRedirect`.

## Verification

- Initial code `2972a5576402a2b14c8fa6b2ccfda308fe2f3bc3` failed rustfmt: [Rust workspace 40718161966](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40718161966). Formatting corrected on master.
- Corrected exact code `a512468167c398fd728924716cf7d3c2eaa4208d`: [Rust workspace 40718286336](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40718286336) **passed** (format, strict Clippy, workspace tests); [Platform shells 40718286448](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40718286448) **passed**; [Android Rust ABI 40718286371](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40718286371) **passed**.
- Regression tests exercise invalid canonical titles and aliases, malformed redirect titles, oversized identities, legitimate multilingual titles and Persian ZWNJ preservation.

## Nonclaims and remaining work

This is a canonical model validation improvement, not proof of publisher-authenticated source completeness, safe HTML sanitization, correct MediaWiki locale-specific title normalization, mandatory offline images/math/attribution, installed pack integrity, production desktop/Android reader navigation or physical-device acceptance. **SEC-001 remains unchecked.** MOD-001 retains its earlier completed qualification; this is additional defense in depth.
