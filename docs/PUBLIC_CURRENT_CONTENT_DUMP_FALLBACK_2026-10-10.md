# Official Wikimedia public current-content export fallback — SRC-003

## Upstream publication contract

Wikimedia's documented [MediaWiki Content File Exports](https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports) offer a public, per-wiki `mediawiki_content_current` export of current revisions, independent of paid Enterprise Structured Contents access. The documented export location is:

`https://dumps.wikimedia.org/other/mediawiki_content_current/<wiki_id>/<YYYY-MM-01>/xml/bzip2/SHA256SUMS`

The `SHA256SUMS` publication is the completion signal for a particular wiki/date export. Do not infer completion from a directory, an early shard or a guessed release filename. The inventory lists relative file paths and SHA-256 digests. Downloaded files must be independently hashed and compared before any content is accepted. A matching hash only establishes consistency with the acquired checksum file; authority depends on retrieving that inventory from the expected Wikimedia publication origin.

The upstream data contains **unparsed XML/wikitext**, not high-fidelity rendered article HTML. It is therefore not a drop-in replacement for revision-matched HTML, category, redirect, template, Lua or MathML processing. Wikimedia notes that its older legacy XML dump production path is being deprecated, particularly for large wikis; do not design this fallback around an assumption that a legacy `pages-articles` file always exists.

## Implemented increment

`wiki-source::public_dump::parse_sha256sums` parses bounded GNU-style SHA-256 inventories, normalizes digest case, rejects malformed checksum lines, path traversal, absolute/ambiguous paths, Windows separators and case-colliding duplicates, and returns deterministic lexically ordered members. The parser does not fetch files, authenticate the source, decompress XML or mark a snapshot ready. `wiki-source::public_dump_verify::verify_dump_member` separately rehashes a bounded caller-supplied byte stream against the parsed member digest. It does not download data or establish source authenticity.

A second partial helper, `wiki-source::public_dump_stage::verify_staged_export`, verifies every declared shard on disk under a private staging root. It rejects symlinked paths, missing or altered shards, and oversized streams. This is not upstream authentication, XML parsing, snapshot activation or protection against a concurrently malicious writer of the staging directory.\n\n## Remaining acceptance work

1. Discover completed official exports without assuming dates; preserve the exact publication URL, wiki ID and release date.
2. Integrate official-origin download, resume and staged activation with bounded streaming SHA-256 verification for every member.
3. Stream/decompress bzip2 XML safely, retaining page IDs, namespaces, revision IDs, redirects, deleted-page state and content provenance.
4. Render templates, Lua, math and references with a compatible MediaWiki/Parsoid renderer and revision-match HTML/category/redirect joins; reject fidelity gaps.
5. Add representative public dump fixtures and end-to-end pack/read acceptance, including mandatory offline images and attribution.

**Qualification boundary:** SRC-003 remains unchecked until the importer, renderer and public-dump article acceptance are demonstrated. This document records a defensible public source contract, not completed fallback support.

## Extractor regression and known limitations (2026-10-11)

The synthetic XML regression suite confirms that properly escaped reference-like wikitext survives XML decoding intact, including `&lt;ref&gt;` and `&amp;`. The exact-head test commit `288bebadfcac6ff269745c97a769ba04375d9cf2` passed Rust workspace CI 38125786299 and Platform shells CI 38125786265.

**Unresolved:** The current `scripts/extract_public_dump_pages.py` scalar-field helper reads `Element.text` without rejecting nested child elements. Unexpected nested XML inside a revision `<text>` field, title, ID, timestamp or namespace may therefore truncate content silently. The importer must reject nested or duplicate scalar XML fields before production acceptance. Escaped markup is valid wikitext and must continue to round-trip. The regression above does not establish acceptance of malformed nested XML. SRC-003 remains unchecked.
