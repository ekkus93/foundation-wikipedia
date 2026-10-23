# SRC-003 — Native XML root-child and siteinfo ordering checks (partial)

Date: 2026-10-23
Status: Synthetic streaming XML parser hardening; SRC-003 remains unchecked.

## Behavior

`wiki-source::public_dump_xml_events::XmlPageMetadataWriter`
now permits only direct `<siteinfo>` and `<page>` children inside
`<mediawiki>`. A single `siteinfo` may precede page records; if
one appears after the first page has begun, the decoder fails closed.
Unknown root-level elements fail closed rather than silently disappear
from the observed census. A valid legacy dump without `siteinfo` is
still accepted. Inner page/revision/contributor ID distinction and
compressed-member provenance gates remain unchanged.

Tests stream synthetic XML with 1-, 3- and 8192-byte input chunks and
exercise an unknown pre-page root child, an unknown post-page child,
a late siteinfo and a valid siteinfo-omitted legacy record.

## Exact-head qualification

- Initial code `f7a523afca43d84bb6c7e3cfdba2536754c54be4` failed
  [Rust CI 39994729756](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39994729756)
  only at rustfmt; source was corrected.
- Qualified code SHA `733fd0218fc9d593ea3e1256af1ebf6a89935a77`
  passed [Rust workspace CI 39994823397](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39994823397)
  (fmt, strict Clippy, tests).
- [Platform shells CI 39994823402](https://github.com/ekkus93/foundation-wikipedia/actions/runs/39994823402)
  was in progress at document creation.

## Limitations

This is a conservative XML semantic restriction, not a conforming
MediaWiki XML parser. Verified digest-pinned compressed bytes and
page-census comparison do not independently prove Wikimedia publisher
identity, completeness, or revision-correct HTML. Actual article wikitext
and template/module dependencies, complete media/attribution and
desktop/Android offline acceptance remain required. SRC-003 stays unchecked.
