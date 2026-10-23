# SRC-003 — Public-dump page identity bidi controls (partial)

Date: 2026-10-22. Repository: ekkus93/foundation-wikipedia.

The Rust `wiki-source::public_dump_page_meta::PageMetadataCollector` now rejects Unicode directional formatting controls U+061C, U+200E–U+200F, U+202A–U+202E and U+2066–U+2069 in both untrusted page titles and redirect targets. Ordinary Arabic and Hebrew titles/redirects remain valid.

A unit regression iterates the rejected characters over both title and redirect events, verifies that a rejection permanently invalidates the collector, and exercises valid right-to-left identities. The first source SHA `ea4ca61aedc6242741fee4c994a04ab506e68af9` failed Rust formatting in run 39895829433; repair SHA `5d50b8a1252dc6b7b68165683168ba341fd43e78` passed Rust workspace run 39896231781 and Platform shells run 39896231754.

This is metadata-event hardening only. It is not a production XML/bzip2 decoder, authenticated Wikimedia publication, independently complete page census, faithful renderer, required-media inventory, or physical offline acceptance. SRC-003 and SRC-005 remain unchecked.
