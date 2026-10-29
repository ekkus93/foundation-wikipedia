# PACK-001 / TRUST-003 — Unicode path alias collision preflight (2026-10-29)

## Scope

Provisional `Manifest::validate` now detects Unicode-lowercase-equivalent object paths, in addition to ASCII case aliases, before producing a canonical identity transcript. The same normalized set is used to detect file-versus-directory collisions independent of input order. This prevents distinct manifest objects such as `media/Étage.svg` and `media/étage.svg` from claiming destinations that can collide on case-insensitive filesystems.

The implementation uses Rust Unicode `to_lowercase()` as a conservative cross-platform preflight. It is **not** a full filesystem-specific Unicode case-fold/NFC equivalence proof, and does not replace safe staging/extraction.

## Regression

`rejects_unicode_case_equivalent_paths_and_directory_aliases` covers duplicate Unicode case aliases, reversed input order, a nested path under an existing file alias, transcript failure and distinct accented/unaccented names. Exact-head Rust workspace CI exercises formatting, strict Clippy and tests.

## Remaining

Publisher signatures, full hostile import fuzzing, mandatory offline visual/media completeness, installed-pack activation and desktop/Android device acceptance remain open. **PACK-001 and TRUST-003 remain unchecked.**
