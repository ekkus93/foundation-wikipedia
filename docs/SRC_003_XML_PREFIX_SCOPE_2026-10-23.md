# SRC-003 — Scoped XML namespace prefix validation (partial)

Date: 2026-10-23  
Status: Qualified narrow XML namespace syntax hardening; **SRC-003 remains unchecked**.

## Change

`wiki-source::public_dump_xml_events::XmlPageMetadataWriter` now keeps a bounded per-element namespace-prefix scope in lockstep with the existing open-tag path. Namespace declarations apply to the entire opening tag irrespective of attribute order. Unqualified elements and attributes remain permitted, and the predefined `xml` prefix continues to work without a declaration.

Opening tags are rejected when element names or non-declaration attribute names use an undeclared prefix, malformed multiple colons, or the reserved `xmlns` prefix as an element name. A declaration inside one element does not leak into its siblings or parents. The previously qualified checks on default-namespace rebinding and reserved XML namespace URI/prefix bindings are preserved.

The implementation accepts declared foreign extension elements without interpreting them as Wikimedia page/revision metadata. It is deliberately not a complete XML Namespaces expanded-name resolver.

## Qualification

- First implementation `323f460aff90544ba7e81e00ff454388dde39060` failed Rust formatting [run 40005718743](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40005718743). Intermediate formatting repair `3cc706e545651432213ba4457ae745d6877bec3e` also failed rustfmt [run 40005815704](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40005815704).
- Corrected exact source SHA `d90067edd1fc206685180a3d8d7b7200ce9822f5` on `master` passed [Rust workspace CI 40006063557](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40006063557) (rustfmt, strict Clippy, tests) and [Platform shells CI 40006063613](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40006063613).
- Synthetic positive cases: a declared prefixed extension element, a qualified attribute whose declaration occurs later in the same tag, and the implicit `xml:space` usage.
- Negative cases: undeclared element/attribute prefixes, a prefix declared only in a closed sibling, an empty namespace binding, a malformed multi-colon element name, and `xmlns` used as an element prefix. Chunk sizes 1, 2, 7 and 8192 bytes.

## Still required

The fallback must authenticate Wikimedia publication and independent expected-page census, qualify complete page/namespace/cutoff semantics against real dumps, preserve exact-revision rendered article and required offline media/attribution, and pass desktop/Android real-device acceptance. Prefix scope checks alone do not establish source authenticity or rendering completeness. Do not mark SRC-003 complete.
