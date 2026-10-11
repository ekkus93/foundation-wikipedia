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

This helper constructs the **fixed Wikimedia HTTPS origin** for the newer `mediawiki_content_current` publication, refuses redirects, bounds the response to 32 MiB, validates the complete page-range checksum inventory and atomically publishes it without overwriting an existing file. Wikimedia documents that the official `SHA256SUMS` marker appears only after the generation is complete. The JSON output records the exact source URL, manifest SHA-256 and member count; `shards_verified: false` explicitly indicates that referenced XML bytes have not been downloaded or verified. TLS origin validation is not a separate signed publisher attestation. No fallback mirror or legacy publication is discovered by this helper.

Synthetic HTTPS-response acceptance (including 404/not-yet-completed, wrong final origin, invalid dates and malicious inventories) runs with:

```bash
python3 -m unittest discover -s scripts -p 'test_discover_public_current_export.py'
```

Qualified at `78b62ee650cbb0e1f94982523133254f0c66a0b0`: [Rust CI 38135015909](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38135015909), [Platform CI 38135015954](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38135015954). Live upstream transfer, complete shard staging, renderer fidelity and pack/device acceptance are not yet qualified.


## Official shard staging transport (partial SRC-003)

After independently verifying the official `SHA256SUMS` origin, create a private staging directory and download the exact listed shards:

```bash
mkdir -p /private/wikimedia-staging
python3 scripts/stage_public_current_shards.py \
  /private/official-SHA256SUMS enwiki 2026-10-01 \
  /private/wikimedia-staging
```

The transport refuses redirects, requires the expected official HTTPS URL for every member, bounds compressed bytes, verifies SHA-256 before publishing each file, and rechecks existing shards before reuse after restart. Incomplete transfers are discarded rather than published. The destination must be private and not concurrently writable by an adversary. The transport does **not** yet implement byte-range resume within an interrupted shard; it re-downloads incomplete shards.

`python3 -m unittest discover -s scripts -p 'test_stage_public_current_shards.py'` covers verified staging and restart reuse; an explicit local-integrity negative regression remains pending. This does not qualify real upstream full-generation acquisition or rendering; the JSONL and offline pack restrictions above still apply.

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
