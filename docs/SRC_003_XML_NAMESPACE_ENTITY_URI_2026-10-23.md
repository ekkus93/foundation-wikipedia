# SRC-003: XML namespace URI character-reference qualification

**Date:** 2026-10-23  
**Status:** Narrow native XML semantic validation complete; SRC-003 remains unchecked.

## Threat and change

The public-dump metadata writer validates the official MediaWiki default
namespace and the reserved XML/XMLNS namespace URIs. Before this change,
`namespace_scope` compared the raw quoted attribute bytes rather than the
expanded XML attribute value. This allowed a namespace declaration such as
`xmlns:evil="http://www.w3.org/XML/1998/namesp&#x61;ce"` to evade the
reserved-URI comparison, even though its expanded URI is reserved.

The adapter now expands XML character references in `xmlns` and
`xmlns:prefix` attribute values before checking the official default URI
and the reserved `xml`/`xmlns` prefix/URI constraints. Non-namespace
attribute processing and bounded streaming remain unchanged.

## Regression and qualification

Tests cover valid character-reference-encoded official export namespace,
valid encoded implicit `xml` namespace binding, and valid encoded extension
namespace; negative cases reject character-reference-obfuscated reserved
XML and XMLNS URIs, and a malformed `xml` URI. Each fixture is written
in chunks of 1, 2, 7 and 8,192 bytes.

- Initial code SHA `95d76447c48c73614e516e2e2981f1c2ad7b44e1`
  failed only rustfmt in [Rust workspace run 40020880225](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40020880225).
- Corrected code SHA `43a97d92a923e3ecb5eb461d9ac619f5c993ff9f`
  passed [Rust workspace run 40021014158](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40021014158)
  and [Platform shells run 40021014172](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40021014172).

## Remaining release gates

This check is not a complete XML Namespaces implementation, and it does not
authenticate the Wikimedia publication, establish independent expected-page
census completeness, render wikitext with revision-matched HTML, provide
required offline media/attribution or qualify physical desktop/Android
acceptance. Keep SRC-003 unchecked until the full acceptance criteria pass.
