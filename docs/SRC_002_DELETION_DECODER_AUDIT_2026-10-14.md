# SRC-002 deletion decoder review and correction (2026-10-14)

## Finding and fix

The public `wiki-source::enterprise_pinned_deletions::decode_pinned_deletions`
previously accepted repeated page IDs with matching project/generation scope and
a matching digest, despite its documented promise to reject duplicate records.
The downstream joined importer already rejected duplicates, but the public
decoder alone could return an ambiguous marker list.

Source commit `e39c15c1661c1212987a962c79f64e6a0dc76999` adds decoder-level
`HashSet` tracking and a typed `DuplicateRecord` error, before returning any
validated marker list. The downstream importer test was updated to expect
the earlier rejection. Public-API regression commit
`23966ed8cc34f972b73db74c1fca41dc822df468` verifies that duplicate
page IDs with an otherwise valid SHA-256 pin fail closed.

## Exact-head qualification

- Code/test SHA: `23966ed8cc34f972b73db74c1fca41dc822df468`
- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38693843718 — passed, including formatting, strict lint and Rust tests.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38693843670 — passed at the same exact SHA.
- The canonical TODO remains 1,536 lines, blob `ec594ed49cf4fff5e1c7ab42fca927881be55df5`; no changes to its checkboxes.

## Qualification boundary

This proves a local, caller-pinned deletion decoder rejects duplicate page IDs.
It does not authenticate upstream Wikimedia publication, independently establish
complete deletion coverage, qualify mandatory article media or prove end-to-end
offline pack/device acceptance. **SRC-002 remains unchecked.**
