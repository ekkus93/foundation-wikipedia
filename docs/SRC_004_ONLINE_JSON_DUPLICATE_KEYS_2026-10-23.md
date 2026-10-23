# SRC-004 — Strict online MediaWiki JSON boundary (2026-10-23)

## Scope

The online Action API navigation/search and revision-pinned rendered-article parsers now share the recursive duplicate-object-key rejecting JSON decoder already used for Enterprise source imports. This closes the ambiguity where serde_json::Value silently kept only the last value of a repeated key, including JSON-escaped spellings of the same key. The guard applies to nested unknown metadata and snippet objects, not only identity fields.

Changed parsers:
- `online_search::parse_search_results`
- `online_title::parse_title_resolution`
- `online_article::parse_current_revision` and `parse_rendered_revision`

Tests reject duplicate page IDs, revision IDs, title/text identities, repeated top-level query objects and duplicates in otherwise ignored nested fields; valid UTF-8 article/search responses remain accepted. Invalid input returns the existing typed `InvalidResponse` error, never a selected article identity.

## Limits

This is a parser-integrity slice of SRC-004, **not** a claim of completed online/offline reader acceptance. Source authenticity still depends on the HTTPS transport and official origin, and revision-accurate required media, persistent installed catalogs, real desktop/Android integration and airplane-mode validation remain outstanding. SRC-004 remains unchecked in the canonical TODO.

## Qualification

Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` on the exact committed SHA; record exact-head CI outcomes before treating this slice as qualified.
