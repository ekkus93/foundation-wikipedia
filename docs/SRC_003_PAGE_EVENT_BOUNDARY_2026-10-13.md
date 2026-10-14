# SRC-003: Streaming dump page/revision event boundary

**Status:** Qualified source validation primitives; not a production XML/bzip2 importer.

The authoritative public-dump importer must only emit `PageEvent` values from a proven well-formed MediaWiki XML parser. The parser must distinguish an outer `<page><id>` from nested `<revision><id>` and contributor IDs, cap decompressed input/output, handle XML entity/DTD hazards, ensure exactly one `<revision>` in a current-content record, and preserve rendered content and revision provenance.

Use `PageMetadataCollector::push` to validate typed events and obtain `PublicDumpPage` only on a complete page. Feed the resulting outer page ID to `StreamShardAudit::push_page_id` for the verified shard. Any error from either state machine disqualifies the entire member; do not continue and ignore an error. The metadata collector rejects duplicate identity fields, partial revisions, duplicate revisions, invalid UTC timestamps, dangerous oversized strings, and out-of-order events.

Passing these checks does **not** authenticate a source, prove it contains all required pages, preserve revision text, provide Parsoid-compatible HTML, or establish mandatory media/attribution. Those requirements remain in `docs/WIKIPEDIA_AI_READER_TODO.md` as SRC-003 and subsequent pack/store tasks.

Qualification: [Rust CI 38493887910](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38493887910), [Platform CI 38493887863](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38493887863); and the page metadata collector at exact SHA `51a87af3fb70616cca79d75da159c45aef6cc647` with [Rust CI 38495578613](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38495578613) and [Platform CI 38495578641](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38495578641).
