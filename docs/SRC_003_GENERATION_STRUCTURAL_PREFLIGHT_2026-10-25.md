# SRC-003: Cheap whole-generation preflight before full SHA-256 (2026-10-25)

The host-only `wiki-pack verify-public-members` command now walks the
complete pinned staging tree and validates all declared paths, types, hardlink
counts, compressed member byte limits and aggregate generation budget **before
reading any compressed member for full SHA-256 verification**. This prevents a
missing late shard, abandoned checkpoint, unlisted file, or oversized shard
from forcing expensive hashing of earlier terabyte-scale members. The pinned
inventory is still verified before scanning.

Only after the tree passes does the command reopen and fully hash every
member against the pinned `SHA256SUMS`; size changes between the structural
scan and digest pass are rejected. Tests prove that structural faults take
precedence over an earlier corrupted member, while digest corruption is still
rejected when the tree is otherwise clean. The staging root must remain
private and free from concurrent untrusted writers.

This is an acquisition integrity/efficiency improvement, **not** publisher
authentication, independent Wikimedia completeness, revision-matched
rendering, required offline visuals, or desktop/Android acceptance.
`SRC-003` remains unchecked.

## Exact-head qualification

- Implementation and test corrections culminated at exact source SHA `32bc011824980d279b3cc428a51427be4fbb1e79`.
- Rust workspace (format, Clippy and unit tests): [40204441763](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40204441763) — passed.
- Platform shells: [40204441824](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40204441824) — passed.
- Prior corrected fixture source `048385cd8f98d0219c62e2701bbbd2a9553d4386` failed rustfmt [40204160605](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40204160605); repaired before qualification.
