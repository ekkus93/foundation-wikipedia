# STORE-003 — Staged candidate durability barrier and state-isolation path

**Date:** 2026-11-12  
**Repository:** `ekkus93/foundation-wikipedia`  
**Code SHAs:** `57a957e10cf177cc2637c98555e9a37fe7aeef63` (barrier), `1900b48cce5c9994ccbd0855c5513c3c1b755b8f` (isolation integration test), `39974030ffc7b965224911955b3daebf327512bc` (SIGKILL harness), `15eae27b` (index replacement ENOSPC)

## Issue

`ActiveSnapshotStore` made only the `current` selector durable: it fsynced the private selector bytes, renamed it over `current` and fsynced the snapshot root. Nothing flushed the staged candidate itself. After a power loss, a durable selector could therefore name `snapshots/<id>` whose manifest, catalog, search index or article shards — or the directory entries naming them — had never left the page cache. No production staging writer exists yet (only tests create candidates), so the switch is the only place that can enforce the ordering for every caller.

## Implementation (`57a957e1`)

- `switch_with_precommit_and_syncs` adds an injectable candidate-sync seam beside the existing selector-directory seam; the previous `switch_with_precommit_and_sync` delegates with real `File::sync_all` so existing fault tests keep their exact sync-call counts.
- After the precommit gate and a staging recheck, `sync_staged_candidate` walks `snapshots/<id>` breadth-first in sorted order, fsyncs every regular file, then fsyncs every directory in reverse order (children before the directory naming them), then `snapshots/` and the root. Staging is checked again before the atomic rename.
- `DirEntry::file_type` does not follow links. A symlink or special file anywhere in the candidate fails closed with the new `ActivationError::UnsafeCandidateEntry`, since its bytes are not owned by the installer's tree. A manifest swapped for a symlink at precommit still reports `CandidateNotStaged` because the staging recheck runs first.
- Every public switch — `switch_to_prevalidated`, the bound-pack-media paths and `switch_to_manifest_verified_with_bound_pack_media` — reaches the barrier.

## Regression evidence

Unit tests in `activation::interruption_tests` (all three failed against a seam with no barrier behind it, then passed with the barrier):

- `staged_candidate_tree_is_durable_before_selector_publication` — with `current` still reading `old`, every staged file, nested shard directory, the candidate, `snapshots/` and root are synced exactly once, each before its parent.
- `candidate_sync_failure_preserves_selector_and_bookmarks` — `ErrorKind::StorageFull` injected at each of the nine sync points, for both first activation and an existing selector: the selector bytes are unchanged, no `.current.`/`.previous.` private files remain, staged bytes are retained, a separate `user-state/user.sqlite` bookmark is intact, and the same candidate activates once storage recovers.
- `candidate_tree_with_symlink_entry_is_never_published` (Unix) — a symlinked shard is refused and `old` stays selected.

Integration test `crates/wiki-store/tests/snapshot_state_isolation.rs` (`1900b48c`) runs the acceptance path with real catalogs, article shards, persisted FTS indexes, bound pack media and a separate user-state database:

1. A corrupted candidate shard fails `switch_to_manifest_verified_with_bound_pack_media`; fresh store/reader/user-state handles (standing in for a restarted process) still select and read `old` revision 1001 and see the same bookmark, reading position and setting.
2. The repaired candidate activates; `old` stays independently readable for rollback; user state is unchanged.
3. The active `search.sqlite` is overwritten with garbage, so the verified reader refuses it. `replace_persisted_from_verified_shards` rebuilds it from the snapshot's own catalog and manifest-verified shards. Search works again, and the SHA-256 of the shard, catalog, user database and both media objects is unchanged — no content, media or user state is re-imported.

Process-interruption harness `crates/wiki-store/tests/activation_kill_harness.rs` (`39974030`, Unix): the test binary re-executes itself as a child that alternates `new`/`old` activations through the real barrier while committing a user-state counter and reading position; the parent SIGKILLs it at 24 staggered delays (0–5 ms after a first completed activation). After every kill, fresh handles must find a selector naming a known snapshot that passes `open_with_manifest_shards` and returns that snapshot's revision; both snapshot directories retained; `user.sqlite` reopening with its bookmark; and a committed counter strictly greater than after the previous kill (progress, no lost commit). Leftover private files must not block later activations. A local run: 24 kills, 99 committed activations, 23 orphaned private selector files, 0.47 s; three repeated runs passed. The child signals readiness with a marker file because a re-executed libtest child's stdout was not flushed before SIGKILL, which hung the first version of the harness.

Derived index replacement (`15eae27b`): `replace_persisted_from_catalog` delegates to an internal variant with an injectable sync seam. `search_index::identity_tests::index_replacement_sync_failures_never_expose_a_partial_index` (Unix) injects `StorageFull` at the staged-file sync and the pre-rename directory sync and proves the prior index bytes are unchanged with no `.offline-fts.` staged file left; at the post-rename directory sync the error is reported, the new index passes verified opening and search, and a retry succeeds with exactly three syncs.

Local verification (Debian 13, Linux 6.12, Rust 1.85.1, `umask 022`): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `python3 -m unittest discover -s scripts` (with `scripts/requirements-rag003.txt` installed) and `python3 scripts/verify_fixture_provenance.py` all passed.

CI on exact SHAs:

| SHA | Rust workspace | Platform shells | Android Rust ABI | Android record codec probe |
| --- | --- | --- | --- | --- |
| `57a957e1` | [43359246014](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359246014) passed | [43359246070](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359246070) passed | [43359245945](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359245945) passed | [43359245903](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359245903) passed |
| `1900b48c` | [43359616535](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359616535) passed | [43359616528](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359616528) passed | [43359616650](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359616650) passed | [43359616505](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43359616505) running at documentation |
| `39974030` | [43360244620](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360244620) passed | [43360244421](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360244421) passed | [43360244552](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360244552) passed | [43360244699](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360244699) running at documentation |
| `15eae27b` | [43360441653](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360441653) passed | [43360441772](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360441772) passed | [43360441652](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360441652) passed | [43360441648](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360441648) running at documentation |

## Boundaries and remaining work

- Injected sync errors and fresh handles are deterministic fault and restart simulation. The SIGKILL harness proves recovery from abrupt process death only: the kernel keeps data a killed process already wrote, so it is **not** proof of survival across power loss, kernel crash or `ENOSPC` raised by a real filesystem. Power-loss qualification needs a block-level fault layer (for example dm-flakey or LazyFS, which require root) or physical power cuts on target devices.
- Every kill that lands between private-file creation and cleanup leaves one `.current.*`/`.previous.*` file in the snapshot root (23 after 24 kills locally). By existing policy these are never adopted, overwritten or deleted, so they accumulate without bound across repeated crashes. Safe reclamation needs an explicit recovery step under exclusive ownership; deciding a file's writer is dead from its PID is unreliable because PIDs are reused.
- After a post-rename directory-sync failure, `replace_persisted_from_catalog` reports an error although the new verified index is already published. That is acceptable for derived data (a retry is safe), but callers must not assume the previous index survives such an error.
- `File::open(dir).sync_all()` is Unix behaviour; Windows cannot open or flush directories this way. The pre-existing selector durability path has the same limitation.
- The barrier flushes the candidate tree only. Durability of the shared media object store and registry is governed by `media_install`/`media_ownership`, not by this change.
- Hard-linked files other than `manifest.json` are not rejected; exclusive ownership of the snapshot root by the installer remains a precondition, as does caller authentication of the manifest and shard/media inventories.
- A production staging installer, a process-kill or power-cut harness, desktop/Android filesystem qualification and device acceptance are still required. **STORE-003 stays unchecked.**
