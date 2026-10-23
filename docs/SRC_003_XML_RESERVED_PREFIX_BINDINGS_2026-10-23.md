# SRC-003 — Reserved XML namespace prefix bindings (partial)

Date: 2026-10-23  
Status: Qualified narrow XML semantics hardening; **SRC-003 remains unchecked**.

## Implementation

The native Rust `wiki-source::public_dump_xml_events::XmlPageMetadataWriter` now rejects namespace declarations that violate the reserved XML prefix/URI bindings, including:

- `xmlns:xml` bound to anything other than `http://www.w3.org/XML/1998/namespace`.
- `xmlns:xmlns`, or another prefix bound to `http://www.w3.org/2000/xmlns/`.
- A non-`xml` prefix bound to the reserved XML namespace URI.
- Empty or malformed prefixed namespace declarations (including extra colons in the prefix).

Valid implicit `xml:space`, an explicitly correct `xmlns:xml`, ordinary `xmlns:xsi`, and the previously validated MediaWiki default namespace remain accepted. The check runs on opening tags at every nesting depth, not just the document root. The earlier default-namespace rebinding rejection remains intact.

## Qualification

- Initial code SHA `2d408dd7c5e994e5c435d9595e9f231458069079` failed Rust format in [run 40004213314](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40004213314); test case formatting was corrected without changing the binding policy.
- Qualified exact code SHA: `19910e5f426d6f272bb0ad8404d180a2154e9dff` on `master`.
- [Rust workspace CI 40004301653](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40004301653): success, including rustfmt, strict Clippy and workspace tests.
- [Platform shells CI 40004301612](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40004301612): success.
- Synthetic regressions cover valid and forged bindings with 1-, 2-, 7- and 8192-byte input chunks.

## Remaining limitations

This is not a complete namespace-aware XML parser, and it does not prove that self-declared namespace URIs or input bytes originated from Wikimedia. The production fallback still requires authenticated source publication, independently authenticated expected census and namespace/cutoff coverage, revision-matched MediaWiki/Parsoid rendering, required offline media and attribution, and desktop/Android real-device acceptance. Do not mark SRC-003 complete based on this check.
