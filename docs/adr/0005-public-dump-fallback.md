# ADR — Official public-dump rendering fallback

**Status:** Implemented fallback path; representative live revision rendering qualified on 2026-10-10.

## Decision

Use verified Wikimedia public-dump XML/XML.bz2 as the source-of-record for
project, page ID, exact revision ID/timestamp, redirect metadata and wikitext.
For selected articles that need browser-fidelity HTML, obtain the exact same
revision from the official MediaWiki REST route:

`https://<language>.wikipedia.org/w/rest.php/v1/revision/<revision-id>/with_html`

The renderer verifies the returned revision ID and, when processing an imported
dump record, its timestamp before accepting HTML. Dump provenance and renderer
provenance remain separate fields. The rendered HTML receives its own SHA-256.

This endpoint is MediaWiki/Parsoid rendering, so template expansion, Lua,
math/reference rendering and other wiki-context behavior are delegated to the
same rendering stack used by the wiki instead of approximated with a
lightweight local wikitext parser.

## Scope and tradeoffs

- This is a **selected-article/topic-pack fallback**, not a claim that the
  public REST service is suitable for rendering an entire Wikipedia snapshot.
  Full-snapshot/high-throughput builders should evaluate a locally operated
  MediaWiki + Parsoid stack populated with matching wiki state.
- It requires network access while a pack is built, but it does not require a
  paid API or Wikimedia Enterprise subscription. Once rendered content/assets
  are packed, reader use remains an offline concern.
- Only explicitly mapped Wikipedia database IDs are accepted. Standard two- or
  three-letter language DB names and `simplewiki` map to canonical
  wikipedia.org origins; irregular historical DB names fail closed until an
  explicit mapping is defined.
- Redirects away from the exact REST URL, mismatched revision IDs/timestamps,
  unexpected content types, invalid JSON, empty/oversized HTML, malformed raw
  dump provenance and excessive Retry-After values fail closed.
- HTTP 429/503 retries are bounded to three attempts with Retry-After capped at
  30 seconds. NDJSON publication is atomic and no-clobber.

## Source fidelity boundary

The verified dump member checksum remains evidence for the imported wikitext.
The REST response is independent rendering evidence for that exact revision;
it does not replace or retroactively authenticate the dump. This separation
prevents a current-page HTML response from being confused with the imported
revision.

The fallback does not yet solve mandatory Commons media acquisition, asset
licensing, offline CSS/font closure or full pack construction. Those remain
PACK-003/PACK-004 concerns.

## Qualification

Exact implementation head `fc65c1b8560706d9463c599461481f7a13a68452`:

- Rust workspace CI 38077397995: success, including
  `test_render_public_dump_revision.py`.
- Wikimedia revision HTML probe 38077398043: success against documented
  English Wikipedia revision 764138197, verifying the official exact-revision
  URL, returned revision identity, non-empty HTML and a recorded HTML SHA-256.
- Platform shells CI 38077398028 is the same exact head and is recorded as
  qualification once terminal.

MediaWiki documentation defines both revision-specific HTML retrieval and the
Parsoid wikitext-to-HTML transform contract. The implementation deliberately
uses revision-specific retrieval so imported dump identity cannot drift to a
newer page revision.
