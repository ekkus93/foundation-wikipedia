# SRC-003: XML expanded attribute-name identity

**Date:** 2026-10-23  
**Status:** Scoped streaming XML semantic hardening; SRC-003 remains unchecked.

## Problem and implementation

The XML nesting guard rejects duplicate lexical attribute names, but XML
Namespaces also prohibits duplicate *expanded* attribute names. Two
different prefixes may be declared to refer to the same namespace URI,
so `a:hint` and `b:hint` are duplicates when `xmlns:a` and
`xmlns:b` resolve to the same URI. Previously, the page metadata writer
tracked only the presence of prefixes, not their URI values, so this
invalid construction could pass semantic parsing.

The metadata writer now stores each declared prefix's decoded URI in
the lexical scope map. Each opening tag checks all non-namespace-declaration
attributes for a unique `(namespace URI, local name)` pair, accounting
for implicit `xml` bindings, unprefixed attributes, same-tag declarations,
inherited prefixes and XML character references. The depth and token-size
bounds remain unchanged.

## Regression coverage

Positive tests preserve attributes with distinct namespace URIs and
different local names sharing one URI. Negative tests reject two aliased
prefixes with the same local attribute name, including character-reference
encoded aliases and bindings inherited from the root. All fixtures are
written in 1-, 2-, 7- and 8,192-byte chunks.

- Initial code SHA `e4c30a66aab8b4b1033ee9c0662df66371810d4b`
  failed formatting in [Rust run 40021637291](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40021637291).
- Corrected code SHA `5d25c40a1966f9cc234426a9a4906f765dab1f07`
  passed [Rust workspace 40021760824](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40021760824).
- [Platform shells 40021760766](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40021760766)
  remained in progress at this observation; check its terminal result.

## Unmet SRC-003 acceptance

These checks do not prove full XML parser conformance, publisher-authenticated
Wikimedia source generation, independently authenticated and compatible
expected census, revision-matched rendered content, mandatory offline
images/math/captions/attribution or desktop/Android acceptance.
**Do not mark SRC-003 complete.**
