# SRC-003 — Reject raw CDATA terminator in XML character data (2026-10-23)

## Scope

XML 1.0 forbids the literal sequence `]]>` in ordinary character data; it may only terminate a CDATA section. The existing incremental `XmlNestingGuard` validated markup nesting, predefined/numeric entities and several forbidden constructs, but accepted this invalid sequence in plain text.

`crates/wiki-source/src/public_dump_xml_structure.rs` now tracks the last two raw right brackets in ordinary text and rejects a following `>` across arbitrary `Write` calls. Markup and entity boundaries reset the sequence. CDATA remains parsed in its separate state. A new regression tests two forbidden forms with 1-, 2-, 3- and 8192-byte input chunks, plus legal escaped and CDATA alternatives.

## Evidence

- Initial implementation `a18a76bd73d62712fe83ea7a7f785efb2b224943` failed the Rust formatting stage because a struct field used a semicolon instead of a comma (Rust CI run 39965738288).
- Corrected implementation `23cacc0a34d07cb6fb0c024d2fee084f5c1e6322` passed Rust workspace CI run 39965834236, including formatting, strict Clippy and unit tests.
- Platform shells CI run 39965834037 was in progress when this note was prepared; do not interpret it as a pass without a terminal success result.

## Qualification limits

This fix narrows one XML 1.0 grammar gap. The envelope guard remains a conservative subset, not a conforming MediaWiki XML semantic parser. The current bzip2 path and this test do not prove publisher-authenticated source identity, full multi-member completeness, independent page census, revision-matched HTML, mandatory offline media or desktop/Android acceptance. No SRC-003 checkbox is completed.
