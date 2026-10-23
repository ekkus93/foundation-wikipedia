# SRC-003 — Native public-dump XML namespace identity guard (partial)

Date: 2026-10-23  
Status: Qualified narrow semantic hardening; **SRC-003 remains unchecked**.

## Change

The portable `wiki-source::public_dump_xml_events::XmlPageMetadataWriter` now validates default XML namespace declarations on opening tags before accepting page/revision metadata. A root default namespace must be a conventional MediaWiki XML export URI (`http://www.mediawiki.org/xml/export-0.N/`, with a bounded numeric minor version). Any descendant default-namespace rebinding is rejected, preventing a foreign namespace's `page`, `title` or `id` elements from being silently treated as Wikimedia records. Root namespace-free synthetic fixtures remain accepted, but are **not** authenticated source data. Ordinary prefixed namespace declarations (such as `xmlns:xsi`) are unaffected.

The new regression tests exercise real-looking root namespace declarations, wrong export URI, foreign root, descendant page/title overrides, and chunk sizes of 1, 2, 7 and 8192 bytes. This semantic check composes with the existing streaming XML envelope, UTF-8, compressed digest, revision and census guards. It is not a general namespace-aware XML parser and does not claim to authenticate the root's URI string.

## Exact-head qualification

- Implementation source SHA: `97eeb3a2ab740719c51851bf1ab971240981c780` on `master`.
- [Rust workspace CI run 39995809421](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39995809421) — **success**, exact source SHA (format, strict Clippy and tests).
- [Platform shells CI run 39995809034](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39995809034) — **success**, exact source SHA.
- Change was committed directly on `master` through Ralph Bridge; no branch or PR was created.

## Still required

A production-qualified fallback must authenticate the actual Wikimedia publication and an independently sourced expected census, establish complete namespace/page/cutoff semantics against real dumps, render complex MediaWiki wikitext with revision-matched fidelity, preserve all required offline media and attribution, and pass desktop/Android offline and physical-device acceptance. A namespace URI in an XML file is only self-declared metadata, not proof of Wikimedia authorship. Do not mark SRC-003 complete based on this slice.
