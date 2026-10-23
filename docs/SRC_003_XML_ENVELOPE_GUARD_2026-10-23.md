# SRC-003 — Streaming XML markup-envelope guard (partial qualification)

**Date:** 2026-10-23  
**Repository:** ekkus93/foundation-wikipedia  
**Code SHA:** `01a37b65938e1310fd434160f920a0427cc69546`  
**Status:** scoped implementation only; SRC-003 remains unchecked.

## Implementation

Added `crates/wiki-source/src/public_dump_xml_structure.rs` with `XmlNestingGuard<W>`, a bounded incremental `Write` sink that observes XML markup while preserving provisional decoded output. It accepts a conservative ASCII-element-name subset compatible with the expected Wikimedia dump envelope. It requires exactly one closed, nonempty `<mediawiki>` root and at least one non-self-closing immediate child `<page>`; it tracks matching opening/closing names, validates quoted and nonduplicate attributes, handles comments and CDATA across arbitrary chunk boundaries, and allows processing instructions before the root. It rejects DTDs, mismatched tags, truncated markup, duplicate roots, unquoted/duplicate attributes, markup exceeding 64 KiB and nesting deeper than 64 elements. Failed writes permanently poison the guard.

Added `decode_checked_member_with_xml_envelope`, which composes the structural guard with the existing incremental `XmlTextGuard`, bounded decoded-output writer, compressed-input digest and EOF checks. The wrapper always finalizes both guards after the decoder callback; a decoder swallowing a malformed-document write error cannot return a success receipt. Added `CheckedMemberError::InvalidXmlDocument` for this composition.

## Validation

Synthetic regression tests cover multilingual document text, one-byte and larger chunks, comments containing `>`, CDATA, mismatched/unclosed tags, duplicate roots, forbidden DTD, empty/self-closing pages, trailing text, duplicate/unquoted attributes, swallowed structural errors and malformed/truncated UTF-8.

- Initial implementation `9de1c86037ef7ea52a63d540c7308a5d9a1a42fe` failed rustfmt: [Rust workspace run 39962205666](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39962205666).
- Corrected implementation `01a37b65938e1310fd434160f920a0427cc69546` passed [Rust workspace run 39962320195](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39962320195) and [Platform shells run 39962320191](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39962320191). The Rust workflow covers rustfmt, strict Clippy and workspace tests.

## Remaining gates and limitations

This is **not a conforming XML 1.0 parser**. In particular it does not validate entity-reference grammar or namespaces, extract or interpret page/revision metadata, or implement an actual bzip2 decoder. The callback must supply verified decompression and complete XML/page semantic validation, and the enclosing import must authenticate the official publication and reconcile an independently obtained complete page census before activating any snapshot. No real Wikimedia dump was decoded in these tests. MediaWiki/Parsoid rendering, mandatory offline images/math/attribution and desktop/Android acceptance remain unqualified. The emitted bytes are provisional and must not be installed or advertised as complete offline content.

**Decision:** retain SRC-003 unchecked; next implement a portable bzip2 + conforming XML reader, integrate with `PinnedPageIngest`, and qualify against real authenticated official exports and independent coverage evidence.
