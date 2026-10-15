# SRC-003 — Public-dump siteinfo project consistency audit

Date: 2026-10-15
Status: Implemented and exact-head CI qualified as a partial SRC-003 safeguard.

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

## Implemented and qualified

- Diagnostic decoder implementation: `scripts/public_dump_xml_census.py` now
  checks any present `siteinfo/dbname` against the declared source project
  before discarding siteinfo and before publishing the census. Duplicates,
  foreign-namespace lookalikes, nested values and mismatches fail closed.
- Regression suite: `scripts/test_public_dump_siteinfo_project.py` exercises
  matching, absent and malformed declarations using compressed XML members,
  verified staging and the actual no-overwrite publication path.
- Qualified exact source SHA: `2cb11bc558247d26fab07eabe8732b685eeb5f84`.
- Rust workspace CI (including Python unittest discovery):
  https://github.com/ekkus93/foundation-wikipedia/actions/runs/38701582900
  — passed on the exact source SHA.
- Platform shells CI:
  https://github.com/ekkus93/foundation-wikipedia/actions/runs/38701583003
  — passed on the same exact source SHA.

This qualifies only optional local siteinfo identity consistency; it does
not authenticate the publication or satisfy the remaining SRC-003 criteria.
The authoritative TODO remains unchecked and unmodified.

## Follow-up: real calendar date for public current-content generations

The diagnostic `stage_observed_census` entrypoint now validates its
`generation_id` as a strict ISO calendar date before accepting a staged
public current-content generation. Previously a manifest, official-shaped
URL and shard filename could agree on an impossible date such as
`2026-02-30`. This is a local source identity consistency check; it does
not prove that a release exists or that Wikimedia published its bytes.

The `test_public_dump_siteinfo_project.py` regression suite also rejects
invalid February, month 13, year zero and non-padded dates before any
census is published.

- Qualified exact code-and-test SHA:
  `934f08125490a474be39b2a733f9bbfddd1870a3`.
- Rust workspace CI:
  https://github.com/ekkus93/foundation-wikipedia/actions/runs/38702105730
  — passed, including Python regression discovery.
- Platform shells CI:
  https://github.com/ekkus93/foundation-wikipedia/actions/runs/38702105652
  — passed at the same exact SHA.

The original SRC-003 production importer, upstream publication authentication,
independent expected census, revision-consistent rendering, required offline
media and desktop/Android acceptance remain outstanding. SRC-003 is unchecked.
