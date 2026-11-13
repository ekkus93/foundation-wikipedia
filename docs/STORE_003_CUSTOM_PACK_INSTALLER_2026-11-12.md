# STORE-003 — Custom `.wpack` installer into an inactive snapshot

**Date:** 2026-11-12  
**Repository:** `ekkus93/foundation-wikipedia`  
**Code SHAs:** `bd3b8104` (installer, `crates/wiki-store/src/pack_install.rs`), `6d1e51b8` (staging reclamation and SIGKILL install harness)

## Issue

Pack staging (`wiki_pack_format::stage`) and snapshot activation (`wiki_store::activation`) existed, but nothing produced `snapshots/<id>` in production: every candidate was created by tests. STORE-003's "staged snapshot activation" therefore had no installer feeding it.

## Owner decisions (2026-11-12)

1. **Crate ownership:** `wiki-store` owns the installer and takes a dependency on `wiki-pack-format` (which does not depend on `wiki-store`, so there is no cycle).
2. **Layout:** a pack's objects mirror the snapshot layout directly — `catalog.sqlite`, the article shards the catalog names, and optionally a derived `search.sqlite`. There is no translation step.
3. **Trust before TRUST-001:** custom packs that pass integrity verification may be installed after explicit user confirmation. Official-origin claims are refused until publisher signatures can be verified.

## Implementation (`bd3b8104`)

`install_custom_pack` (seekable, input byte budget) and `install_custom_pack_stream` (unseekable Android document-provider stream, bounded spool) share one path:

1. Create `snapshots/` and a private `staging/` (0700) under the caller-owned root; refuse a symlinked root, ancestor or either directory.
2. Stage through `stage_container_with_budget` / `stage_container_stream_with_budget` into `staging/install-<pid>-<nanos>-<n>`: two-pass envelope verification, every object rehashed, 0700/0600 permissions.
3. Refuse `Origin::Official` with `UnsignedOfficialPack` **before** confirmation. Custom packs are labelled `CustomIntegrityVerified` from the same successful byte verification.
4. Require a `pack_id` that is a valid snapshot ID. Refuse a pack-supplied `manifest.json` (installer-owned), SQLite `-wal`/`-shm`/`-journal` sidecars and `.offline-fts.*` staging names; require `catalog.sqlite`. Every other object is a manifest-pinned shard.
5. If the pack omits `search.sqlite`, rebuild it from the manifest-verified shards. Then open the staged tree with `VerifiedOfflineCatalogReader::open_with_manifest_shards`: catalog shard coverage must equal the pinned shard set exactly, each shard rehashes and the index matches the catalog. An unlisted object therefore fails closed.
6. Write the installer-owned `manifest.json` (envelope identity, origin, trust label, object inventory), fsync it and the staged root, and refuse an existing `snapshots/<pack_id>` (`AlreadyInstalled`; never replaced).
7. Call the caller's `confirm(&PackInstallReview)` with the manifest, trust label, declared bytes and whether the index was rebuilt. `false` returns `Declined`.
8. Rename the staged tree to `snapshots/<pack_id>` and fsync `snapshots/` and `staging/`.

Every failure in steps 3–8 before the rename, including a declined confirmation, removes the staged tree and syncs `staging/`. Installation never touches `current`: activation is still a separate `ActiveSnapshotStore` switch, which runs the candidate durability barrier from `57a957e1`. The returned `InstalledPack.shards` is the inventory that `switch_to_manifest_verified_with_bound_pack_media` expects.

## Regression evidence

Unit tests in `pack_install::tests`:

- `verified_custom_pack_installs_inactive_then_activates` — review data reaches `confirm`; the snapshot lands at `snapshots/<pack_id>` with an installer `manifest.json` and an empty `staging/`; the selector stays empty until `switch_to_prevalidated`, after which the verified reader opens the installed revision.
- `omitted_search_index_is_rebuilt_from_verified_shards` — a pack without `search.sqlite` installs with a rebuilt, searchable index.
- `declined_confirmation_leaves_no_snapshot_and_keeps_selector` — with `old` active, declining `new` leaves no snapshot or staging residue and `old` selected.
- `unsigned_official_claim_is_refused_before_confirmation` — a byte-valid `Origin::Official` pack never reaches `confirm`.
- `catalog_shard_mismatch_is_refused_and_rolled_back` — a self-consistent envelope whose catalog does not describe its shard fails verified opening and is removed.
- `layout_violations_are_refused` — a pack `manifest.json`, a SQLite WAL sidecar, an unlisted shard, a missing catalog and an invalid snapshot ID are each refused with nothing published.
- `existing_snapshot_is_never_replaced` — reinstalling an ID reports `AlreadyInstalled` and the original bytes and revision are unchanged.
- `stream_install_matches_seekable_install` — the spooled stream path installs the same tree and leaves no spool file.
- `sync_failures_never_publish_a_partial_snapshot` — `StorageFull` at the staged-root sync removes the tree and a retry installs. At either post-rename directory sync, the error is reported, the complete verified tree is in place but inactive, a retry reports `AlreadyInstalled`, and activation succeeds.

Staging reclamation and process interruption (`6d1e51b8`):

- `reclaim_abandoned_staging(root)` removes only `install-*` directories and `.wpack-spool-*` files from `staging/`, never follows a symlink (an `install-*` symlink is retained and its target untouched), never touches `snapshots/`, and reports `(removed, retained)`. It is an explicit call, because the caller must guarantee no install is in flight. `reclaim_removes_only_install_residue` covers it.
- `crates/wiki-store/tests/pack_install_kill_harness.rs` (Unix) re-executes the test binary as a child that installs 400 prebuilt custom packs (each without a search index, so every install also rebuilds one). The parent SIGKILLs it 16 times at staggered delays after each new publication. After each kill, every directory under `snapshots/` must have an installer `manifest.json` and pass `open_with_manifest_shards`. The number of installed snapshots must strictly increase. Reclamation must then empty `staging/` with nothing retained, and leave the snapshot set unchanged. Finally a surviving snapshot activates. Three local runs passed: 16–17 installed snapshots and 14–16 reclaimed staging entries per run (most kills landed mid-install), about 4.2 s each.

Local verification (Debian 13, Linux 6.12, Rust 1.85.1, `umask 022`): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `python3 -m unittest discover -s scripts` (636 tests, with `scripts/requirements-rag003.txt` installed) and `python3 scripts/verify_fixture_provenance.py` all passed.

CI on exact SHAs:

| SHA | Rust workspace | Platform shells | Android Rust ABI | Android record codec probe | Dependency lock refresh probe |
| --- | --- | --- | --- | --- | --- |
| `bd3b8104` | [43371714498](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43371714498) passed | [43371714525](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43371714525) passed | [43371714597](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43371714597) passed | [43371714486](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43371714486) passed | [43371714463](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43371714463) passed |
| `6d1e51b8` | [43372305517](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43372305517) passed | [43372305366](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43372305366) passed | [43372305289](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43372305289) passed | [43372305383](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43372305383) passed | not triggered |

The Android record codec probe for `39974030` had failed on emulator infrastructure (`adb` exit 224 before the probe wrote its log; the surrounding `1900b48c` and `15eae27b`, which contains `39974030`'s tree, both passed). Its failed job was re-run and passed ([43360244699](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43360244699), attempt 2).

## Boundaries and remaining work

- Pack media are not installed: a pack with objects the catalog does not name is refused. Installing required media into the shared object store (`media_manifest_install`) and binding them to the snapshot owner remain separate.
- `manifest.project`/`snapshot` are recorded but not cross-checked against catalog article keys.
- Existence check then `rename` relies on the documented precondition that the caller owns the root exclusively; on Linux `rename` would replace an empty directory at the target.
- A post-rename sync failure is reported although the snapshot is published (inactive). A crash can leave an `install-*` directory or spool file in `staging/`; it is never adopted and is removed only by the explicit `reclaim_abandoned_staging`, which callers must run while no install is in flight (for example at startup). Orphaned `.current.*`/`.previous.*` selector files in the snapshot root still have no reclaimer.
- Directory fsync is Unix behaviour, as elsewhere in the store. Power-loss qualification, desktop/Android filesystem qualification and device acceptance are still required. **STORE-003 stays unchecked.**
