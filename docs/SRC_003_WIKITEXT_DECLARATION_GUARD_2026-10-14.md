# SRC-003 diagnostic wikitext declaration validation

Date: 2026-10-14
Status: Qualified partial implementation. SRC-003 remains unchecked.

At exact source SHA cd507eb6e481600fe8595c551fd79e643d9360c9, the optional revision-content-jsonl decoder now rejects explicit non-wikitext or ambiguous MediaWiki content model and format declarations. This prevents JSON or Scribunto content from being mislabeled as article wikitext. Older exports without model or format declarations remain accepted; explicitly declared wikitext and text/x-wiki formats are supported for legacy revisions and modern main slots. Tests cover unsupported formats, duplicate declarations, incompatible slot attributes and accepted wikitext.

Exact-head qualification:
- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38674845092 — passed, including Python synthetic regression tests.
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38674845209 — passed.

Limits: This is a diagnostic input-classification guard, not publisher authentication, independent page census, a portable Rust bzip2/XML importer, revision-matched Parsoid rendering, mandatory offline media or a qualified pack. SRC-003 remains unchecked. The authoritative TODO and existing census pipeline qualification document were not modified.
