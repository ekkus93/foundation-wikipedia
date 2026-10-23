# SRC-003 — Native bzip2 → XML page metadata → pinned shard audit

**Date:** 2026-10-23
**Status:** Qualified implementation slice; SRC-003 remains incomplete.
**Exact code SHA:** d143921e5f3d422a7b943786437d40a407b076b0

## Implemented

- The portable Rust streaming XmlPageMetadataWriter consumes bounded XML chunks and emits PublicDumpPage records through PageMetadataCollector, distinguishing page/id, revision/id, and nested contributor/id. It handles escaped title and redirect labels, ordinary metadata, nested revision text, comments, CDATA and XML processing instructions without buffering article text. Duplicate/missing fields, malformed numeric IDs and timestamps, invalid character references and callback failures reject the stream.
- decode_bzip2_xml_page_metadata composes MultiBzDecoder with compressed-input SHA-256 and EOF validation, byte budgets, incremental UTF-8/XML character and conservative XML-envelope checks, and the new semantic metadata writer.
- ingest_pinned_bzip2_member feeds each decoded record into PinnedPageIngest for revision-date, page-range, uniqueness and ordering checks. It aborts the audit if decompression, XML validation, metadata, EOF or checksum fails, even after earlier provisional page callbacks.
- All operations remain streaming; revision wikitext is not accumulated. Callback records and counters are **provisional** until all independent source and pack-activation checks succeed.

## Automated qualification

Exact SHA d143921e5f3d422a7b943786437d40a407b076b0:

- Rust workspace CI (format, strict Clippy, unit tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/39980574567 — success.
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39980574741 — success.
- New synthetic tests exercise byte-by-byte XML event extraction, Unicode and entity decoding, nested contributor IDs, duplicate/missing fields, poisoned callbacks, actual bzip2-compressed XML, revision-scoped page identity, shard auditing, and checksum-mismatch poisoning.
- Earlier corrected, qualified component SHAs: b47d5595cdcfd7299277517e84db9b49798c8681 (streaming semantic writer), 01b8bc55964d4c16c3b9d72fe3f753373ee8be30 (native bzip2 composition).

## Outstanding requirements and limitations

This is not a complete MediaWiki dump importer. The XML envelope/parser accepts a conservative subset and does not implement general namespace-aware XML semantics. A full official-source import must also prove publisher-authenticated generation and checksum origin, independent source/page census completeness across all shards, robust normalization and high-fidelity MediaWiki/Parsoid rendering, complete required offline images/math/captions/attribution, safe staged activation and real desktop/Android offline acceptance.

Never treat callback delivery, a single member receipt, or a successful page-ID audit as a publishable snapshot. Continue to leave SRC-003 unchecked in the canonical TODO.
