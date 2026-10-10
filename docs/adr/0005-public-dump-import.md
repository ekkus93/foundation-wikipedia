# ADR — Public current-content XML fallback (provisional)

**Status:** Importer prototype; SRC-003 remains unchecked (2026-10-10).

The public Wikimedia `mediawiki_content_current` export is discoverable via
official completed SHA256SUMS inventories. The staging pipeline validates
downloaded compressed members against that inventory. The new
`scripts/import_public_current_content.py` reader accepts a staged
`.xml.bz2` member and its expected SHA-256, hashes compressed bytes before
parsing, rejects unsafe file types, DTD prologs, duplicate page IDs, malformed
revision records, deleted text and truncated XML/bzip2.

It yields page ID, exact current revision, namespace, title, timestamp,
redirect target and raw wikitext. Unicode and redirects are regression-tested.
A caller **must exhaust the iterator successfully before activating a staged
snapshot**; a late parse failure invalidates the entire staged import.

This is **not** a rendered or installable Wikipedia fallback yet. Remaining
work includes official-dump fixture qualification, streaming/disk-backed
duplicate indexing for multi-million-page exports, bounded XML parser resource
use, revision-scoped canonical normalization, and MediaWiki/Parsoid-compatible
rendering of templates, Lua, math, citations, tables and required visuals.
No HTML fidelity or offline-pack acceptance is claimed.
