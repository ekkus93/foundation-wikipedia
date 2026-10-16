# PACK-002 — Optional related-article topic preview (partial)

Date: 2026-10-16
Status: Qualified synthetic CLI integration only; PACK-002 remains unchecked.

## CLI contract

`wiki-pack preview-topic --json <local-category-graph.json>` accepts an optional `related` member alongside existing `snapshot_id`, `roots`, `categories`, `include`, `exclude`, `redirects`, `depth_limit` and `page_limit`.

```json
{
  "snapshot_id": "local-untrusted-snapshot",
  "roots": ["Physics"],
  "depth_limit": 1,
  "page_limit": 100,
  "categories": [{"name": "Physics", "articles": [10]}],
  "related": {
    "max_hops": 1,
    "max_new_pages": 5,
    "max_edge_visits": 100,
    "links": [{"from": 10, "to": 11}]
  }
}
```

Without `related`, output and the category selection SHA-256 retain the existing contract. With `related`, `selected_page_ids`, `selected_page_count` and `selection_sha256` reflect the expanded selection. `category_selection_sha256` preserves the original category-only digest. `related_added_page_count`, `related_link_mentions` and `related_source_authenticated: false` explain the optional expansion. The original `input_sha256` fingerprints exact caller-supplied JSON bytes.

The expanded SHA-256 is derived deterministically from a domain-separated tuple containing the base category-selection digest, configured expansion limits, canonicalized and sorted related-link adjacency, and final sorted page IDs. Reordering equivalent links does not alter this canonical digest. Redirect identities and exclusions are applied before traversal. A cycle or an exceeded link, page, hop or edge budget fails closed rather than returning a partial selection.

## Limits and provenance

This command only previews caller-supplied local JSON. It does **not** verify that the category graph, related links or snapshot label originated from Wikimedia; it does not verify official source completeness, mandatory offline visuals, publisher signatures or installability. A preview fingerprint is not a publisher signature or a `.wpack` manifest receipt.

The standalone `wiki-related-preview` diagnostic remains provisional. Production PACK-002 still requires authenticated snapshot-pinned related-link ingestion, integration into the shared pack builder and final page-ID manifest, category/related definition persistence, media completeness and cross-platform acceptance.

## Qualification

Implementation SHA: `916a8ed348cee89ea587f0a1322c6ed7e0fb4a77`.

- Rust workspace [38983071581](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38983071581): passed, including synthetic integration tests for link-order determinism, canonical exclusions, backward compatibility and fail-closed limits.
- Platform shells [38983071545](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38983071545): passed.
- Intermediate authored CI failures and separate platform safety denials are documented in [Ralph Bridge write and CI incidents](RALPH_BRIDGE_WRITE_AND_CI_INCIDENTS_2026-10-15.md).

No external Wikimedia source authenticity, end-to-end pack build, offline media or real-device acceptance is claimed.
