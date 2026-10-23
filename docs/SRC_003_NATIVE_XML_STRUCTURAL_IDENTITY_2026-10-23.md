# SRC-003: Native XML structural identity validation

**Date:** 2026-10-23  
**Scope:** Partial native XML metadata acceptance, not full SRC-003 qualification.

## Change

`crates/wiki-source/src/public_dump_xml_events.rs` now rejects placement
of structural `mediawiki`, `siteinfo`, `page` and `revision` tags
outside their permitted MediaWiki current-content dump positions. A
standalone metadata writer also refuses a second root (including another
concatenated document), a foreign root name, and an unclosed root at EOF.
An attacker-supplied page/revision nested inside an otherwise ignored
metadata subtree is not silently accepted as unrelated extension content.

The writer still extracts *provisional page identities*, not rendered
article text or trusted offline content. The outer XmlNestingGuard,
compressed member checksum receipt, ordered page audit and staged census
must continue to qualify independently.

## Regression evidence

- Implementation SHA: `befea069c6273cd45e0cc48e5c12efcd3190633a`.
- Native streaming tests reject nested page under siteinfo, nested revision
  and siteinfo under contributor, nested root, foreign root, and two
  concatenated documents.
- Positive regression accepts two consecutive legitimate page elements
  within one MediaWiki document and preserves their stable page and
  revision IDs.
- All fixtures use 1-, 2-, 7- and 8,192-byte incremental writes.
- [Rust workspace exact SHA 40037337275](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40037337275) — passed rustfmt, Clippy and Rust tests.
- [Platform shells exact SHA 40037337267](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40037337267) — in progress at documentation; confirm terminal result.

## Outstanding acceptance

This is *not* publisher authentication, independent expected-page census,
revision-matched MediaWiki/Parsoid rendering, required offline images/math/
captions/attribution, a qualified pack build, or desktop/Android real-device
acceptance. SRC-003 stays unchecked.
