# SRC-003 — Checked compressed member plus XML text composition (partial)

Date: 2026-10-23
Status: Synthetic Rust boundary qualified; SRC-003 remains unchecked.

## Change

Added `wiki_source::public_dump_xml_text::decode_checked_member_with_xml_text`.
It composes the pinned compressed-member SHA-256/EOF guard, a bounded
provisional decoded-output writer, and `XmlTextGuard`. Unlike a decoder-owned
guard, the wrapper always invokes `finish()`, even when the decoder returns
an error. It refuses to return the provisional output if a decoder swallows
invalid UTF-8, an XML 1.0 forbidden character, or a trailing incomplete
multibyte sequence. The new `CheckedMemberError::InvalidXmlText` distinguishes
this validation failure from an I/O writer failure.

Compressed checksum mismatch, early return before compressed EOF, and decoded
byte-budget exhaustion still fail closed. A successful callback must drain the
compressed input and must route all decoded bytes through the supplied writer.
The output is provisional: an outer authenticated, complete multi-member
import must qualify before publication.

## Regression evidence

Three synthetic tests exercise valid multilingual XML text, ignored write
errors, truncated UTF-8, mismatched compressed SHA-256, unconsumed compressed
trailing bytes and decoded output budget exhaustion. The compressed fixture
is deliberately not bzip2 and no real Wikimedia dump was decoded.

Initial code SHA `c4e75f9576da5461fceb6b3f57600be6ee0d894b`
failed rustfmt: Rust workflow
https://github.com/ekkus93/foundation-wikipedia/actions/runs/39954835598 .
Corrected exact code SHA: `7d4b7f6d47b2fc05190d55658b11cab1222a3791`.

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39954920826 — success (fmt, strict Clippy, tests).
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39954920907 — pending at documentation time.

## Not yet implemented

The wrapper does not implement a bzip2 decoder, an XML tokenizer or structural
validator, authenticated upstream source completeness or independent expected
census, MediaWiki/Parsoid rendering, mandatory offline media/attribution,
or physical desktop/Android acceptance. It must not be represented as a
completed public-dump fallback. `SRC-003` stays unchecked.
