# SRC-004 / SEC-001 — cache title noncharacter integrity (2026-10-24)

## Scope

The revision-keyed temporary HTML cache already rejected control characters, selected bidirectional format characters, and invisible separator/format markers in displayed titles. It did not reject Unicode's designated noncharacters, which are invalid as stable public-facing article title identifiers. The title validator now refuses U+FDD0–U+FDEF and code points U+*FFFE/U+*FFFF at the end of every Unicode plane. This applies to both new insertions and reads of persisted SQLite entries, even when the persisted row has been changed outside the cache API.

A synthetic Rust regression checks rejection of U+FDD0, U+FDEF, U+FFFE, U+FFFF, U+1FFFE and U+10FFFF on writes and on tampered-row reads. Existing tests preserve legitimate Persian ZWNJ and Indic ZWJ in titles. The cache remains revision-keyed and does not declare content current or offline-complete.

## Exact-head evidence

- Initial code SHA `256877bc1a81f42e417b49276fe2d51ec1e32479`: [Rust workspace 40130714060](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40130714060) failed the format gate because of a spurious blank line introduced at a file-read chunk boundary.
- Corrected code SHA **`e9cf0cf7fcd3eed3e9a70472277631d54a4ae66e`**: [Rust workspace 40130760388](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40130760388) passed formatting, strict Clippy and workspace tests; [Platform shells 40130760361](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40130760361) passed; [Android Rust ABI 40130760349](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40130760349) passed. The [Android record codec probe 40130760370](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40130760370) was in progress at observation and is not required evidence for cache title validation.

## Qualification boundary

This closes one synthetic cache-metadata validation gap. It does not authenticate publisher responses, guarantee live API interoperability, sanitize rendered HTML, enforce all language-specific Wikimedia title canonicalization, prove required offline media completeness, or qualify desktop/Android physical-device behavior. **SRC-004 and SEC-001 remain unchecked.**
