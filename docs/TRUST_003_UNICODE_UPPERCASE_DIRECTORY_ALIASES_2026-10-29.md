# PACK-001 / TRUST-003 — Unicode uppercase and directory-alias protection (2026-10-29)

## Finding

The provisional `.wpack` manifest validator rejected identical lowercase object paths, but Unicode lowercasing alone does not identify every case-insensitive filesystem alias. Greek small sigma (`σ`) and final sigma (`ς`) both uppercase to `Σ`, while long s (`ſ`) and ASCII s uppercase to `S`. Furthermore, two distinct objects could use differently cased spellings of the same intermediate directory (such as `media/É/a.svg` and `media/é/b.svg`) and silently merge on a case-insensitive filesystem.

## Implementation

`crates/wiki-pack-format/src/manifest.rs` now rejects duplicate object paths under either Unicode lowercasing or uppercasing. It also tracks each path prefix in both forms and rejects distinct literal spellings of the same case-equivalent directory. File-versus-directory collision detection runs against both fold sets. This check runs during manifest validation, before any staging or transcript acceptance. It is intentionally conservative and does not rewrite a signed manifest identity.

Regression tests exercise Greek sigma/final-sigma, long-s/ASCII-s, case-equivalent intermediate directories with different leaf files, reversed input order, and distinct valid Greek names.

## Verification

**Exact code SHA:** `1bc3ea106f7e57b9c58a98c8afec45863cb7fa73`

- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40915583974) (format, strict Clippy and tests).
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40915583930).
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40915583936).

These tests establish portable manifest preflight behavior, **not** complete Windows/macOS filesystem semantics or real-device installation. Publisher signatures, mandatory media completeness and atomic pack activation remain unqualified. **PACK-001 and TRUST-003 remain unchecked.**
