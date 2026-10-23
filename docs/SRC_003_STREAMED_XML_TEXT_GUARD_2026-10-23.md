# SRC-003 — Streamed decoded XML character guard (partial)

Date: 2026-10-23
Status: Qualified synthetic component only; SRC-003 remains unchecked.

## Scope

Added `crates/wiki-source/src/public_dump_xml_text.rs` and exported
`XmlTextGuard<W>`. It validates decoded UTF-8 incrementally across arbitrary
writer boundaries and checks XML 1.0 legal character ranges. A split UTF-8
scalar is retained in at most three pending bytes and must be completed
before `finish()` can return the provisional writer and byte count.
Invalid UTF-8, XML-forbidden controls, writer errors, and unfinished
sequences fail closed; a rejected write permanently poisons the guard.
The guard processes at most 8192 caller bytes per `write` call.

The writer can already contain a valid prefix when validation fails.
Its output MUST be private/uncommitted. Callers must invoke `finish()`
and still independently validate XML document structure, compression
integrity, page semantics, official publisher provenance, source census,
and complete offline media. A caller that drops the guard without calling
`finish()` has no qualification receipt. The guard does not by itself
decompress bzip2 or parse XML tags/entities.

## Regression evidence

Synthetic unit tests cover UTF-8 split at every byte boundary in multilingual
XML (Persian ZWNJ, Indic ZWJ, Japanese and supplementary-plane characters),
forbidden XML control bytes, overlong UTF-8, surrogate encodings, out-of-range
Unicode, noncharacters U+FFFE, truncated multibyte sequences, swallowed
sink failure, and composition with `decode_checked_member_with_bounded_output`.
The composed fixture is intentionally NOT bzip2 and makes no real-dump claim.

Initial code SHA `a5d1ccc3a10ea6279b71cabb5dc4058f4c7cd002`
failed strict Clippy in Rust workflow 39947485859 due to
`map_err` versus `inspect_err`. Corrected exact code SHA:
`54c43ac492db186e9422f8a04f1cdbac93779f05`.

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39947591819 — success.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39947591838 — success.

## Outstanding SRC-003

Native portable bzip2 decoding, a full XML structural/semantic parser,
authenticated official source inventory and independent census,
MediaWiki/Parsoid rendering fidelity, mandatory offline media/attribution,
and desktop/Android offline-device acceptance remain outstanding.
