# Public current-content dump: integrity versus page coverage

**Scope:** SRC-003 source fidelity guard, 2026-10-13. This is a partial validation capability; it does not qualify an offline importer.

## Upstream contract and known risk

Wikimedia's new `mediawiki_content_current` XML/bzip2 publication is monthly, on the first day of the month. A release is discoverable once its exact `SHA256SUMS` file exists at `https://dumps.wikimedia.org/other/mediawiki_content_current/<wiki_id>/<YYYY-MM-01>/xml/bzip2/SHA256SUMS`. Each checksum entry names a release member. These checksums protect transport integrity but do not independently establish that the producer included every eligible page.

Sources: [Wikitech export contract](https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports), [Wikimedia phabricator T431872](https://phabricator.wikimedia.org/T431872), [Wikimedia phabricator T432178](https://phabricator.wikimedia.org/T432178). The reported July 2026 export omitted thousands of pages edited between snapshot cutoff and later generation, although shard checksums matched. Treat that report as an upstream fidelity concern, not proof that every generation is affected.

## Deterministic local guard

`wiki-source::public_dump_coverage::audit_page_coverage` accepts two pre-sorted, unique, strictly positive page-ID sequences. Both records must declare the same project, release-generation ID and namespace. It uses a linear merge walk with constant auxiliary memory apart from at most 12 example IDs per difference class, and rejects any missing or unexpected IDs.

Expected IDs must come from an **independently qualified** source with comparable snapshot cutoff, project, namespace and deletion/redirect semantics (for example a reconciled authoritative namespace/page listing). Copying IDs from the same suspect dump, relabeling a different cutoff or comparing with a stale index cannot prove fidelity. This API checks equivalence of supplied censuses, not independence, origin, completeness of the expected census, revision correctness, template rendering or offline media availability. Neither `SHA256SUMS` validation nor a successful census comparison may be used alone to declare a pack verified or ready.

## Next implementation requirements

1. Implement bounded current-content XML/bzip2 streaming with secure parser, page/revision/redirect metadata and mainspace filtering; preserve exact generation, timestamp and source identity.
2. Produce an observed sorted unique page-ID census from actually imported revisions and obtain an independently authenticated counterpart with documented cutoff semantics; reject absent/extra identities and reconcile expected deletions/namespace changes.
3. Evaluate source fidelity against representative historical and live samples including the published July 2026 missing-page bug class, template/Lua/math/reference rendering and metadata joins.
4. Do not activate an offline snapshot before all required visuals, media ownership, source revision joins, license attribution and integrity checks pass.

## Qualification

- Rust exact SHA: `916ded44aa3497d8061d6178cf0a26108475bd84`.
- Rust workspace: [38484490968](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38484490968), passing attempt 2; first attempt had unrelated intermittent curl-test `SpawnFailed`.
- Platform shells: [38484490990](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38484490990), passing.
- Fixtures cover equality, missing IDs with otherwise valid hypothetical checksums, unexpected IDs, bounded diagnostics and malformed/incomparable input. No live XML import or independent census acceptance test is claimed.
