# STORE-003 — Explicit reclamation of orphaned private selector files

**Date:** 2026-11-12  
**Repository:** `ekkus93/foundation-wikipedia`  
**Code SHA:** `0cb6a029`

## Issue

A snapshot switch writes the new selector to a private `.current.<pid>.<n>.tmp` file and, when a selector already exists, a synced `.previous.<pid>.<n>.tmp` backup. It then renames the private file over `current`. A process killed between creating either file and its cleanup leaves it in the snapshot root. By policy these files are never adopted or overwritten, and until now nothing removed them either. They therefore accumulated without bound: the SIGKILL harness from `39974030` left 20–25 after 24 kills.

The owner decided on 2026-11-12 to add explicit cleanup. STORE-003 stays unchecked.

## Implementation (`0cb6a029`)

`ActiveSnapshotStore::reclaim_orphaned_selector_files() -> Result<(removed, retained), ActivationError>`:

- **Refuses a corrupt selector.** It first reads `current` through the stable selector read. A corrupt selector returns `CorruptCurrentPointer`, and the leftovers are kept for inspection and manual recovery.
- **Removes only the switch's own files.** It deletes regular files whose names exactly match `.current.<digits>.<digits>.tmp` or `.previous.<digits>.<digits>.tmp`, each number being 1–20 ASCII digits. `DirEntry::file_type` does not follow symlinks. Symlinks, directories and other `.current.*`/`.previous.*` names are counted as retained and left untouched.
- **Makes removal durable.** It fsyncs the snapshot root once if anything was removed.
- **Checks the selection afterwards.** It re-reads `current` and fails if the selection changed, as defense in depth.

**Why removal is safe:** an orphan is never an active selector. `current` changes only by an atomic rename, and rollback consumes the `.previous.*` backup by renaming it back over `current`. Removing an orphan therefore cannot change which snapshot is selected. A first activation killed before its rename leaves the store unselected, just as before the attempt.

**Preconditions:** the caller must guarantee no switch is running against the root, for example by calling this once at startup. Liveness is never inferred from the PID in the name, because PIDs are reused. The cleanup is an explicit call rather than part of every switch, because a concurrent in-process switch's file is indistinguishable from an orphan.

## Regression evidence

Unit tests in `activation::reclaim_tests`:

- `private_selector_names_match_only_switch_output` covers accepted names, including the `u64::MAX` counter. It also covers rejected names: non-numeric, missing or extra segments, empty segments, sign characters, a 21-digit segment, wrong suffix and other prefixes.
- `reclaim_removes_switch_orphans_without_changing_selection` uses three switch-style orphans, one of them naming the inactive snapshot, which must be removed and not adopted. An operator file, a matching-name directory and (Unix) a matching-name symlink to an outside file must all be retained. One root sync is required. `old` stays selected, the symlink target and both snapshots are untouched, a second call removes nothing, and a later switch works.
- `reclaim_before_first_activation_keeps_store_unselected`: an orphan from a killed first activation is removed and `current()` stays `None`.
- `corrupt_selector_is_refused_and_orphans_kept_for_recovery`: a selector naming a missing snapshot is refused and the backup bytes are unchanged.
- `reclaim_sync_failure_is_reported_with_selection_intact`: an injected `StorageFull` on the root sync is reported, the selection is unchanged, and a retry is clean.

`crates/wiki-store/tests/activation_kill_harness.rs` now ends by reclaiming. After 24 SIGKILLs and two further switches, reclamation retains nothing and leaves no private files. `old` stays selected and passes the full recoverability audit: verified opening, both snapshots retained, the bookmark and the committed counter. Three local runs reclaimed 25, 25 and 20 orphans.

Local verification (Debian 13, Linux 6.12, Rust 1.85.1, `umask 022`): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `python3 -m unittest discover -s scripts` (636 tests) and `python3 scripts/verify_fixture_provenance.py` all passed.

CI on exact SHA `0cb6a029`:

| Rust workspace | Platform shells | Android Rust ABI | Android record codec probe |
| --- | --- | --- | --- |
| [43375180098](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43375180098) passed | [43375180082](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43375180082) passed | [43375180079](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43375180079) passed | [43375180080](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43375180080) passed |

## Boundaries

- Cleanup runs only when the caller calls it; nothing automatic invokes it. Desktop and Android startup wiring does not exist yet.
- Exclusive ownership of the snapshot root remains a caller obligation; this cannot detect a concurrent switch in another process.
- As elsewhere, this is process-interruption evidence, not power-loss evidence, and directory fsync is Unix behaviour.
- **STORE-003 stays unchecked.** The owner chose on 2026-11-12 to keep it open pending power-loss and device qualification.
