# SRC-003 — Native bzip2/XML siteinfo project consistency (partial)

Date: 2026-10-23
Status: Synthetic regression-validated source consistency; SRC-003 remains unchecked.

## Scope

The native Rust public-dump page metadata decoder now provides a scoped
`decode_bzip2_xml_page_metadata_for_project` entry point, used by
`PinnedObservedCensus::decode_native_bzip2_member`. The expected Wikimedia
project is read from the immutable `PinnedInventory`, not supplied by an
untrusted member transport. If the decoded XML includes
`<siteinfo><dbname>…</dbname></siteinfo>`, its database name must match the
pinned project exactly. Duplicate database fields, duplicate siteinfo
containers and malformed project identifiers fail closed.

Because MediaWiki exports can omit this optional field, its *absence*
remains accepted for backwards compatibility; absence does not authenticate
the source or prove that the shard belongs to the claimed wiki. A valid
matching field is only internal source consistency evidence, not publisher
authentication.

## Tests and evidence

- Split-write (1, 2, 7, 8192 bytes) metadata tests accept matching dbname
  and reject mismatched (frwiki vs enwiki), invalid-case, duplicated,
  duplicate-siteinfo, and malformed expected project identities.
- A genuine bzip2-compressed input is decoded through the scoped API.
- A full native private-staging test verifies a cross-project bzip2/XML member
  returns an error and leaves **zero** published or temporary census files.
- Exact corrected code SHA:
  `fd8a904409b6834b8493dd3987b0000a3e2a920f`.
- [Rust workspace CI 39992869955](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39992869955):
  passed (fmt, strict Clippy, tests).
- [Platform shells CI 39992870049](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39992870049):
  in progress when this note was prepared.

## Recovery / qualification discipline

The initial change `9ff0c4e5b0a27ab911bfa2bf85b885a8854ff1e2`
inadvertently upserted a partial census source file. It was immediately
restored byte-for-byte from the previously qualified SHA at
`35f7799388761512f01ceb8aafab22f82439286d`.
The intended call-site change was reapplied to the complete 933-line
source, and the intact complete file was verified again. Follow-up source
commits `24f3fd75fa0f9f299bbc2d2cbf7e5c55f8c5140e` and
`79a034a554546c0a643bab223576f13741ea63fa`
failed rustfmt before qualification. No task completion is asserted on
those intermediate commits.

## Outstanding

Official Wikimedia HTTPS publication authentication and expected-census
independence/cutoff matching; a fully conforming MediaWiki XML parser;
capturing revision wikitext and matching templates/Lua/math in Parsoid;
mandatory offline media/attribution; pack/renderer/desktop/Android acceptance.

**SRC-003 remains unchecked.** This safety check cannot authorize
activation of a canonical source snapshot by itself.
