# STORE-002: Bound ignored staging entries in orphan audits (2026-10-26)

**Status:** Scoped read-only filesystem-audit hardening; STORE-002 remains unchecked.

The content-addressed media scanner already counts published digest paths and ignores well-formed `.PID.COUNTER.tmp` names left by interrupted no-overwrite publication. Previously the number of ignored staging entries was unbounded: an attacker or malfunctioning installer could populate arbitrarily many staging names and make an otherwise bounded orphan audit traverse them. Worse, a symlink or directory with a staging-looking name would be skipped without checking its filesystem type.

`MediaObjectStore::published_object_digests(max_objects)` now independently caps ignored staging entries at `max_objects` (1–10,000) and rejects staging-looking entries unless `symlink_metadata` identifies a regular file. It never follows staging symlinks or deletes staged bytes. The existing cap on published objects remains unchanged. Callers of `media_gc::plan_unregistered_media_gc` inherit the fail-closed behavior.

Synthetic tests exercise two ignored regular staging files with a smaller and larger bound, verify staging bytes remain unchanged, and on Unix reject symlinked and directory-shaped staging aliases while leaving an external symlink target untouched. This does **not** authorize deletion or solve writer/reader coordination, stale registry snapshots, stale staging cleanup, publisher authentication, complete visual discovery, or physical-device acceptance.

**Exact-head CI evidence:** Implementation SHA `f96c041fc70e264310bba7d518f7a2c1a3659d57`: [Rust workspace 40388406008](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40388406008), [Platform shells 40388406407](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40388406407), and [Android Rust ABI 40388405991](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40388405991) passed. [Android record codec probe 40388406159](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40388406159) was still running at observation. These are synthetic/CI results, not physical-device acceptance.
