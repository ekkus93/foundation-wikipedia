# SRC-003 — Bind direct native bzip2 ingest to pinned project

Date: 2026-10-23
Status: Scoped source-identity improvement; SRC-003 remains unchecked.

## Change

The direct `ingest_pinned_bzip2_member` path previously called the
unscoped `decode_bzip2_xml_page_metadata` parser, which validated
the XML envelope and revision/page metadata but did not compare an
optional `<siteinfo><dbname>` against the pinned inventory's project.
The observed-census path already used a project-scoped decoder.

The direct ingestion path now obtains the project from its
`StreamShardAudit`-backed pinned inventory and invokes
`decode_bzip2_xml_page_metadata_for_project`. The source's
`dbname` is checked when present, without requiring this optional
metadata from legacy fixture dumps. A mismatched database name
poisons the ingest and prevents shard completion.

## Verification

Real bzip2 test fixtures contain the same valid page/revision and
matching pinned compressed digest. A `frwiki` siteinfo field
in an `enwiki` inventory is rejected; a matching `enwiki`
siteinfo field succeeds. The initial test fixture accidentally
encoded a literal backslash-n instead of the checksum file's
required LF terminator; corrected after the failing test report.

- Initial code SHA `bdc6903800d8b69e6576d805828f87078565a28b`:
  [Rust CI 40012996949](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40012996949)
  failed the malformed fixture.
- Corrected code SHA `7e46bc9f453f70f9f3a01562c9b84932fdf43bd0`:
  [Rust workspace CI 40013592453](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40013592453)
  and [Platform shells CI 40013592589](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40013592589)
  passed.

## Scope limitations

This validates a declared, optional XML database identity against
a pinned inventory; it does not authenticate the Wikimedia publisher
or establish source completeness, independent census provenance,
revision rendering fidelity, mandatory offline media or device acceptance.

**SRC-003 remains unchecked.**
