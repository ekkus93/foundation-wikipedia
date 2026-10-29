# PACK-001 / TRUST-003 — Win32 portable path preflight (2026-10-29)

The provisional `wiki-pack-format::Manifest::validate` now rejects Windows-invalid filename characters (`< > " | ? *`) in pack identity components and object path components. It also rejects reserved Win32 device names `CONIN$` and `CONOUT$`, and superscript-digit `COM¹`/`COM²`/`COM³` and `LPT¹`/`LPT²`/`LPT³` spellings, with extensions and case variants. Existing ASCII device-name, traversal, colon, separator, path-length, digest and Unicode spoofing guards remain intact.

The same component validator applies to pack IDs, snapshot IDs, publisher/definition IDs and each archive object path. This is **portable manifest validation**, not proof of safe extraction on every platform.

## Regression

The Rust test `rejects_windows_reserved_names_and_invalid_filename_characters` covers reserved variants, six invalid filename characters, manifest/transcript rejection and a valid portable name. Run `cargo test -p wiki-pack-format`; exact-head CI covers workspace formatting, strict Clippy and tests.

## Outstanding

The provisional container still lacks publisher signature authentication, complete hostile archive/resource fuzzing, renderer-discovered mandatory media and complete desktop/Android install acceptance. **PACK-001 and TRUST-003 remain unchecked** until their full acceptance evidence exists.
