# MOD-002 — conservative soft-anchor relocation (partial)

- Implementation: `crates/wiki-core/src/evidence.rs`, `crates/wiki-core/tests/evidence.rs`.
- Exact implementation SHA: `c794c7a87f0cff948a04350e0505a94027038498`.
- Qualification: exact descendant SHA `db4b001561800df1b90bad427dd9c94ae36e28d1` passed Rust workspace CI https://github.com/ekkus93/foundation-wikipedia/actions/runs/38077258840, Platform shells CI https://github.com/ekkus93/foundation-wikipedia/actions/runs/38077258745, and Wikimedia revision HTML probe https://github.com/ekkus93/foundation-wikipedia/actions/runs/38077258817.
- Behavior: a bookmark/highlight relocates across revisions only when project/page identity matches and exactly one new block has identical content and heading ancestry. Missing, edited, duplicated and cross-page matches fail closed. Media indices and footnote references are not guessed. Hard AI citation IDs remain bound to their original revision.
- Remaining: UI bookmark persistence/relocation, complete canonical evidence lifecycle and citation highlight integration. MOD-002 stays unchecked.
- This evidence file supplements the canonical TODO and does not mark the parent task complete.
