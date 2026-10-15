# SRC-003 — Public-dump siteinfo project consistency audit

Date: 2026-10-15
Status: Identified source-consistency gap; not implemented or qualified.

## Evidence at inspected master

At exact master `f154f3075f8cc4d61c62cc38e876776ffe9681ed`, `scripts/public_dump_xml_census.py`
validates the pinned member name against the manifest's project and generation,
but its streaming XML parser clears a completed `<siteinfo>` subtree without
checking an optional `<dbname>` declaration against that manifest identity.

A valid XML member can therefore declare a contradictory database name while
retaining a matching filename, manifest digest and otherwise valid page IDs.
This is a **local metadata consistency** gap, not by itself evidence of
publisher impersonation or a compromised upstream publication.

## Required next change

When a completed `<siteinfo>` is encountered, reject a present
`<dbname>` if it is duplicated, in a foreign XML namespace, has nested
elements, or does not exactly equal the manifest's declared project. Retain
compatibility with legacy fixtures that omit siteinfo or dbname.

Add deterministic regressions for matching enwiki, mismatching dewiki,
duplicate declarations, foreign-namespace declarations, and absent dbname.
A rejected member must not publish a partial census. Run Python unit tests
and exact-head Rust workspace and Platform shells CI.

This check does not authenticate the dump publisher, prove an independent
complete census, provide a production Rust XML importer, render Parsoid HTML,
or satisfy mandatory offline media and device acceptance. Keep SRC-003
unchecked until those requirements are qualified.
