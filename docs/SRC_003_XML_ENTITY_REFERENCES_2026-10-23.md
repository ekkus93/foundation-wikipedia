# SRC-003 — Incremental XML entity and comment checks (partial qualification)

**Date:** 2026-10-23  
**Exact code SHA:** `7fb3748d6f4b13effd4a8559bce3a921e370e272`  
**Status:** partial; `SRC-003` remains unchecked.

## Implementation

Extended `wiki-source::public_dump_xml_structure::XmlNestingGuard` to validate XML entity references in both streamed text and quoted attributes. The implementation accepts only the five XML-predefined named entities (`amp`, `lt`, `gt`, `apos`, `quot`) and XML 1.0 legal decimal or hexadecimal numeric character references. It rejects unknown entity names, malformed numeric digits, zero, surrogates, out-of-range code points, unescaped ampersands, incomplete references at EOF, and entity references exceeding a 32-byte bound. References spanning decoder writes remain valid. Comment syntax rejects internal `--` not immediately followed by the closing `>`.

The existing `decode_checked_member_with_xml_envelope` wrapper composes this guard with UTF-8/XML-character validation, compressed-input digest/EOF verification and decoded-output bounds. No provisional bytes are publishable until the entire enclosing source import is authenticated and validated.

## Evidence

- Initial SHA `81930114a36e0465bd474a6fa5f227979cc246db` failed rustfmt: [Rust workspace 39963049944](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39963049944).
- Corrected exact SHA `7fb3748d6f4b13effd4a8559bce3a921e370e272` passed [Rust workspace 39963195525](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39963195525) (fmt, strict Clippy, workspace tests) and [Platform shells 39963195592](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39963195592).
- New synthetic tests exercise named/numeric references split at every byte, attribute references, invalid references, Unicode range violations, and illegal comment double hyphens.

## Remaining gates

This parser is a deliberately conservative markup subset, not a fully conforming XML parser. It does not implement namespace resolution, XML declaration/processing-instruction semantics, full page/revision extraction or bzip2 decompression. The tests use synthetic XML, not a real authenticated Wikimedia dump. Official publication identity and independent completeness, MediaWiki/Parsoid rendering, mandatory offline media and physical desktop/Android acceptance are still required. No task completion is claimed.
