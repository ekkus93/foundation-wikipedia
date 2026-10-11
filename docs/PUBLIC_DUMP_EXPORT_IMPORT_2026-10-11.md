# Current-content XML export → revision JSONL (SRC-003 partial)

**Status:** Qualified bounded offline staging, **not** a full Wikimedia source importer, trusted publisher attestation, Parsoid renderer, or installable offline pack.

## Inputs and provenance

Wikimedia documents completed current-content XML exports under:

`https://dumps.wikimedia.org/other/mediawiki_content_current/{wiki}/{YYYY-MM-01}/xml/bzip2/SHA256SUMS`

Only a completed publication should expose `SHA256SUMS`. The file lists each compressed XML member by digest and relative filename, including page-ID ranges. Consult [Wikitech's MediaWiki Content File Exports documentation](https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports) for the publication contract. These source files contain **unparsed wikitext**.

**The program itself does not fetch or independently authenticate this URL.** Before calling it, an acquiring application must pin the intended Wikimedia HTTPS authority and completed generation, obtain the checksum file from the verified official source, and stage **all** checksum-listed compressed members into a private, non-concurrently-writable directory with their relative names unchanged. A local checksum file and self-consistent files alone establish only internal integrity, not Wikimedia origin or completeness.


## Official completed-export inventory discovery (partial SRC-001/SRC-003)

```bash
python3 scripts/discover_public_current_export.py \\
  enwiki 2026-10-01 /private/official-SHA256SUMS
```

This helper constructs the **fixed Wikimedia HTTPS origin** for the newer `mediawiki_content_current` publication, refuses redirects, bounds the response to 32 MiB, validates the complete page-range checksum inventory and atomically publishes it without overwriting an existing file. Wikimedia documents that the official `SHA256SUMS` marker appears only after the generation is complete. The JSON output records the exact source URL, manifest SHA-256 and member count; `shards_verified: false` explicitly indicates that referenced XML bytes have not been downloaded or verified. TLS origin validation and live SHA256SUMS byte comparison are not separate signed publisher attestations. No fallback mirror or legacy publication is discovered by this helper.

Synthetic HTTPS-response acceptance (including 404/not-yet-completed, wrong final origin, invalid dates and malicious inventories) runs with:

```bash
python3 -m unittest discover -s scripts -p 'test_discover_public_current_export.py'
```

Qualified at `78b62ee650cbb0e1f94982523133254f0c66a0b0`: [Rust CI 38135015909](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38135015909), [Platform CI 38135015954](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38135015954). Live upstream transfer, complete shard staging, renderer fidelity and pack/device acceptance are not yet qualified.


## Official shard staging transport (partial SRC-003)

After discovering and retaining the official `SHA256SUMS` inventory, create a private staging directory and download its exact listed shards. The CLI first **re-fetches the exact current official Wikimedia HTTPS `SHA256SUMS` bytes** and rejects local inventory drift, redirects, absent publication or content-encoding changes before any shard I/O:

```bash
mkdir -p /private/wikimedia-staging
python3 scripts/stage_public_current_shards.py \
  /private/official-SHA256SUMS enwiki 2026-10-01 \
  /private/wikimedia-staging
```

The transport preflights the local inventory against the currently published official HTTPS inventory, refuses redirects, requires the expected official HTTPS URL for every member, bounds compressed bytes, verifies SHA-256 before publishing each file, and rechecks existing shards before reuse after restart. Incomplete transfers remain unpublished in private `.part` files for byte-range resume. On restart, the downloader hashes existing partial bytes and promotes a complete checksum-matching partial without a network request, or sends an exact `Range: bytes=N-` request. It requires HTTP 206, the original URL, identity encoding, an exact `Content-Range` through the end of the member and a consistent response byte count; an ignored range or corrupted completed member fails closed. Only checksum-matching bytes are hard-linked to the final filename and fsynced. The destination must be private and not concurrently writable by an adversary; upstream SHA-256 inventory authentication is still a separate precondition.

`python3 -m unittest discover -s scripts -p 'test_stage_public_current_shards.py'` covers verified staging, restart reuse, interrupted range resume, completed-partial promotion, corrupted bytes, redirects, invalid range responses, unsafe symlinks, download budgets and a two-shard discovery-to-JSONL synthetic pipeline. Resume source/test commit `0b5810559009ed934840dce5d7455c996da74f76` passed [Rust CI 38149111548](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38149111548) and [Platform CI 38149111525](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38149111525). The fresh official publication preflight was qualified by synthetic official-response tests at `367dd261bb18384bc5e5176711896229d08dc9c7`: [Rust CI 38149687951](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38149687951) and [Platform CI 38149688005](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38149688005). These tests do not substitute for live publisher/fidelity acceptance. This does not qualify real upstream full-generation acquisition or rendering; the JSONL and offline pack restrictions above still apply.

## Offline local invocation

```bash
python3 scripts/extract_public_dump_export.py \
  /private/wikimedia-staging \
  /private/official-SHA256SUMS \
  enwiki 2026-10-01 \
  /private/output/articles-unrendered.jsonl
```

The destination parent must already exist and the output file must not exist. The importer validates the declared member set, project/date and non-overlapping page-ID partitions; bounds compressed bytes, extracted page counts and output bytes; hashes all compressed shards before extraction; extracts each member with the bounded single-shard importer; rechecks all input hashes; then publishes one combined JSONL through a no-overwrite hard link. A failure before publication leaves no final output. Records preserve project, page ID, exact revision ID/timestamp, namespace, redirect target, wikitext, and source member SHA-256.

The importer supports two current-revision page-range XML/bzip2 layouts: `mediawiki_content_current` (`enwiki-2026-10-01-p1p100.xml.bz2`) and public `pages-meta-currentN` (`enwiki-20261001-pages-meta-current1.xml-p1p100.bz2`). Supply the date as `YYYY-MM-01` for either layout. Mixing layouts in one inventory fails closed. History, multistream and index members are not supported. Page-ID gaps are permitted. Filenames alone do not establish publication authenticity or completeness. Legacy public dumps generally use MD5/SHA-1 manifests; a caller-generated SHA-256 inventory does not independently authenticate legacy Wikimedia publications.

## Reproducible synthetic acceptance

```bash
python3 -m unittest discover -s scripts -p 'test_extract_public_dump_export.py'
python3 -m unittest discover -s scripts -p 'test_extract_public_dump_pages.py'
```

The Rust workspace CI's `Verify fixture provenance` step runs the complete `scripts` unittest discovery at each exact commit. Qualification at `9f6bc709fe046d7692fa84cafa148a9789d04318` passed [Rust CI 38132498464](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38132498464) and [Platform CI 38132498477](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38132498477).

## Not an offline-ready article

JSONL is **not** sanitized, revision-matched rendered HTML; it does not resolve Lua, templates, math, references, infoboxes or attribution. No article may be labeled offline-ready or used for a verified official pack without separate completed publisher-authentic source acquisition, compatible rendering and visual discovery, media object verification, publisher signature policy, and reader/device acceptance. Audio/video payloads remain on-demand rather than pre-bundled; required visual content and attribution must be available offline. Leave SRC-003 open until its end-to-end acceptance is met.
