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
