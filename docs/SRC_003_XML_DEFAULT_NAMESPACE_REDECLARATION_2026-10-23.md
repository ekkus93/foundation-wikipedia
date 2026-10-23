# SRC-003 — Equivalent default XML namespace redeclaration (2026-10-23)

**Scope:** Conservative compatibility fix for official public-dump XML page metadata; SRC-003 remains incomplete.

## Defect and correction

XML Namespace 1.0 permits a child element to repeat the same default namespace URI already declared by an ancestor. The native `XmlPageMetadataWriter` previously rejected every descendant `xmlns` declaration, even an identical URI, causing valid equivalent MediaWiki export markup to be rejected.

The namespace scope now retains the root's official default namespace binding under the empty-prefix key. A descendant `xmlns` declaration is permitted only if it resolves, after XML attribute whitespace normalization and character-reference expansion, to that **exact inherited URI**. A foreign URI, empty URI, different official export version, or new descendant namespace when the root did not declare one remains invalid. Prefixed namespace bindings and duplicate expanded-attribute protections remain unchanged.

## Evidence

- **Code SHA:** `514ec8725dea07e1265d8b17df000f9aad1ded84` on `master`.
- **Rust workspace CI:** https://github.com/ekkus93/foundation-wikipedia/actions/runs/40035661212 — success (rustfmt, strict Clippy, tests).
- **Platform shells CI:** https://github.com/ekkus93/foundation-wikipedia/actions/runs/40035661230 — success.
- **Regression:** `rejects_foreign_and_rebound_default_xml_namespaces` now includes positive identical and entity-escaped equivalent child namespace declarations, and a negative child declaration changing official export version. All cases are streamed in 1-, 2-, 7- and 8192-byte chunks.

This is a semantic XML compatibility check, not publisher-source authentication or proof that every valid MediaWiki XML export is accepted. No live official dump census, revision-matched HTML/media or real-device reader qualification was performed.

**SRC-003 remains unchecked.**
