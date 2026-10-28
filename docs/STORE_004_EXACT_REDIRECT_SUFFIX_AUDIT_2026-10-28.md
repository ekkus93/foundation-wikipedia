# STORE-004 — Exact redirect-title suffix audit (2026-10-28)

## Problem

The prior complete-membership audit checked each FTS row's project, page ID, revision, title and canonical article-body prefix, but permitted any space-prefixed trailing text. An attacker or corrupted derived index could append invented redirect titles or alter genuine ones without invalidating the canonical prefix.

## Implementation

- In `crates/wiki-store/src/search_index.rs`, first traverse every catalog article, comparing the exact canonical text prefix and identity against the corresponding FTS row and recording only its SQLite rowid, consumed UTF-8 byte offset and total body byte length. Preserve the existing 100,000-article cap.
- Traverse verified catalog records again in the same stable order as `rebuild_from_catalog`. For each redirect, resolve its complete chain through `SnapshotCatalog::resolve_title` to a digest-verified canonical article. Enforce the 500,000-redirect cap.
- Read only the expected next byte span of the target FTS row using `substr(CAST(body AS BLOB), ...)`. Compare the exact `" " + redirect.title` UTF-8 bytes and advance that article's offset. This handles multibyte Unicode without conflating SQLite character and byte offsets.
- Reject missing targets, wrong suffix bytes, truncated bodies, excess appended text and mismatched final offsets with `SearchError::StaleIndex`. Keep the complete redirect corpus out of memory.

The existing `open_verified_read_only` and `replace_persisted_from_catalog` call this audit, so the stronger check applies before opening or publishing a verified derived index.

## Regression and CI

The existing multi-edition disk-backed catalog fixture now includes English and French redirect records, an emoji-bearing redirect, and negative mutations that forge a redirect title or append an extra fake redirect while leaving canonical article text intact. Rebuilding from verified immutable frames restores a passing audit.

Initial source `847b7d417f2b96f054c0d5dc239148f327f50859` failed rustfmt (Rust 40765237866). Formatting repair `2ed0a3ae1f367b197eeafd87da4d5259a7e4f7fe` failed compilation because a mutable rowid reference was passed to `rusqlite::params!` (Rust 40765744128). Corrected source `32fad5ad77c687b77796b603832f2293ffafcc60` passed:

- [Rust workspace — 40766042913](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40766042913): formatting, strict Clippy, workspace tests.
- [Platform shells — 40766043105](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40766043105): passed.
- [Android Rust ABI — 40766042928](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40766042928): passed.
- [Android record codec probe — 40766042889](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40766042889): in progress at last observation; do not count as passed.

## Limitations

The stored FTS body and verified canonical article/redirect inventory now agree byte-for-byte, but FTS5's separate inverted token postings are not independently authenticated. The catalog still depends on independently authenticated Wikimedia source publication and a complete mandatory-media manifest. This work does not demonstrate production pack activation, offline UI integration, or real desktop/Android device acceptance. **STORE-004 remains unchecked.**
