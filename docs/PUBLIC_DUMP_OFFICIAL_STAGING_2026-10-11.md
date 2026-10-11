# Completed Wikimedia current-content export staging — SRC-003 partial

## What this does

`scripts/stage_public_dump_export.py` discovers the newest completed
`mediawiki_content_current` publication for a requested wiki using the
official Wikimedia HTTPS origin. It accepts a generation only after its
`SHA256SUMS` completion marker is available, then downloads **every**
checksum-listed XML/bzip2 shard. Every downloaded member must match its
declared SHA-256 and fit the configured byte budget. The complete private
staging directory is published only after all shards pass verification.

Upstream format and completion semantics:
https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports

This is **raw source acquisition**, not publisher signing, rendering,
canonical article normalization, offline media acquisition or pack installation.

## Invocation

```bash
mkdir -p /private/wikimedia
python3 scripts/stage_public_dump_export.py \
  enwiki /private/wikimedia/enwiki-current

python3 scripts/extract_public_dump_export.py \
  /private/wikimedia/enwiki-current \
  /private/wikimedia/enwiki-current/SHA256SUMS \
  enwiki YYYY-MM-01 \
  /private/output/unrendered-revisions.jsonl
```

Replace `YYYY-MM-01` with the actual `date` recorded in
`/private/wikimedia/enwiki-current/PUBLICATION.json`; do not guess the
publication date. Create the output parent first. Both destination paths must
not exist. The staging parent must be a private, trusted directory that cannot
be concurrently modified by an untrusted process.

The staging directory contains the original `SHA256SUMS`, all listed
relative member paths, and a local `PUBLICATION.json` receipt with the
publication URL, inventory SHA-256, member count and compressed-byte total.
The receipt is **not** a signed Wikimedia attestation.

## Fail-closed behavior and limitations

- A corrupt, missing, empty or oversized shard prevents publishing the
  completed staging directory. Failed attempts remove temporary artifacts.
- Official-origin HTTPS and redirect checks prevent arbitrary mirror/host
  substitution. The published SHA-256 list still has no detached publisher
  signature; origin assurance relies on HTTPS and the expected official host.
- The downloader currently fetches whole members without range resume or
  retry/backoff. It does not perform free-disk reservation, and the temporary
  staging directory may require substantial storage. Do not treat this as
  production-grade large-wiki acquisition until those are implemented.
- Aggregate and per-member compressed-byte caps are enforced; source XML
  decompression, record and JSONL caps are enforced separately by the
  downstream extractor.
- No source is activated as an offline snapshot or installed as a pack.
  Wikitext still needs revision-matched MediaWiki/Parsoid rendering,
  templates/Lua/math/references, canonical normalization and complete
  required visual media with attribution. Audio/video payloads are on-demand.
- The directory rename assumes exclusive ownership of the private staging
  parent; it is not an adversarial cross-process directory-creation lock.
  Future repository-scoped execution leases are independent from this
  filesystem trust requirement.

## Qualification

```bash
python3 -m unittest discover -s scripts -p 'test_stage_public_dump_export.py'
python3 -m unittest discover -s scripts -p 'test_extract_public_dump_export.py'
```

Synthetic acceptance exercises complete two-shard staging, checksum failure,
aggregate/per-member budgets, no partial publication, and staging-to-revision
JSONL extraction. The Rust workspace CI invokes the full Python unittest suite.
Live Wikimedia multi-gigabyte download, resume, renderer parity, offline
images/attribution and device acceptance remain **unqualified**. SRC-003
must remain unchecked until all required acceptance evidence exists.
