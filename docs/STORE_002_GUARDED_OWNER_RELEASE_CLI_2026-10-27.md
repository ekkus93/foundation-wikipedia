# STORE-002 — Explicit guarded pack/cache/pin owner-release CLI

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Implementation

Added a separate operator command for revoking an exact pack, cache, or user-pin owner's references and reclaiming only media that becomes unowned:

```sh
wiki-pack media-owner-release --confirm-release /private/wiki/media /private/wiki/owners.sqlite pack my-pack-id 1000
wiki-pack media-owner-release --confirm-release /private/wiki/media /private/wiki/owners.sqlite cache my-cache-id 1000
wiki-pack media-owner-release --confirm-release /private/wiki/media /private/wiki/owners.sqlite pin saved-article-id 1000
```

The command requires the literal `--confirm-release` (no implicit destructive default), a kind exactly `pack`, `cache`, or `pin`, a valid exact owner ID, and a 1–10,000 candidate budget. It opens only an existing initialized SQLite registry, refuses a registry inside the media object root, checks media-root path components against symlinks/traversal, and rejects Unix roots writable by group or others. It invokes `release_bound_owner_and_reclaim_media`, which acquires an EXCLUSIVE SQLite-backed media lease **before** validating the persistent registry-to-media-root binding and then revoking owner claims. It runs the existing bounded, verified, SQLite-first garbage collector without reacquiring the lease. Media still owned by another pack, cache entry, or user pin is never a GC candidate.

A successful command prints the committed owner-claim count and reclaimed object/byte totals. If GC fails **after** owner release commits, the CLI returns a nonzero exit code and a deliberately explicit `OWNER RELEASE COMMITTED` message with cleanup failure details. It is unsafe to interpret that exit as a rolled-back uninstall. Retry **bound media maintenance**, not owner revocation. Releasing an already absent owner is idempotent and reports zero claims.

This command changes **media ownership only**. It does not change the installed-pack catalog, validate a publisher, atomically activate/deactivate snapshots, or coordinate a reader's installed-pack UI state. A future production uninstall transaction must sequence those separate durable states safely.

## Black-box qualification

New binary-level integration tests execute the actual `wiki-pack` dispatcher and verify:

- Missing confirmation, unknown owner kind, invalid candidate limit, unbound registry, and wrong media root reject before any ownership change.
- Two packs sharing a digest plus a user pin preserve that digest through each pack removal; a pack-private digest is reclaimed, and the shared digest is reclaimed only after its final pin is released. A repeated removal is an idempotent no-op.
- A corrupted cache-owned digest causes GC failure after the cache owner is durably removed. The CLI exits nonzero with explicit committed-release wording, the SQLite registry confirms revocation, and independent bound maintenance reclaims the file after repairing the synthetic fixture.

Initial source SHA `04d2015825ab6cf51f6a38e6e94bc7ac5c9bda87` failed formatting at [Rust CI 40551452061](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40551452061). Corrected exact source SHA `d4bfa56d6f90dc79a2ba54c3db0c6b6930e34c1d` passed [Rust workspace CI 40551595474](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40551595474) and [Platform shells CI 40551595393](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40551595393).

## Remaining work

- Wire installed-pack, cache and pin lifecycle transitions to this bound API through a crash-recoverable application state machine, including atomic snapshot deactivation and retry/recovery semantics.
- Qualify actual host/app scheduling and real Android/desktop storage; perform disk-full, SIGKILL, fsync and physical power-loss testing.
- Strengthen protection against hostile same-UID and direct unleased filesystem/SQLite writes; validate publisher provenance, complete offline media inventory and end-to-end device acceptance.

**Do not mark STORE-002 complete.**
