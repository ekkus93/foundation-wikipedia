# STATE-001 — Post-copy SQLite backup validation (2026-10-29)

## Failure mode

`UserState::backup_to_path` previously exported a consistent standalone SQLite image with `VACUUM INTO` and synced the destination, but did not verify that the copied rows satisfied the application's domain invariants. A second writer or a corrupt live database could therefore produce a successful backup containing invalid builder jobs, article identities, chat messages or other user-state records even when SQLite's own integrity check would accept the image.

## Implementation

`crates/wiki-store/src/user_state.rs` now uses one shared `validate_standalone_copy` routine for both backup publication and new-path restore. The validator opens the actual destination read-only and checks the expected version, `PRAGMA integrity_check`, foreign-key consistency, table/column/index structure, and complete persisted-row semantic rules. `backup_to_path` runs it after `VACUUM INTO` and syncing the copy, and removes the newly created destination on failure. The source schema is read before export so a supported standalone v1 backup is accepted as v1 and can be migrated on its later open; a copied version mismatch is rejected.

## Regression and exact-commit evidence

- `backup_rejects_invalid_copied_rows_and_cleans_destination`: after startup, an independent invalid builder-job status is inserted into the live database. Backup returns `InvalidValue`, leaves no destination, and succeeds after the row is repaired; the reopened backup retains the saved bookmark.
- Existing `restore_of_v1_backup_migrates_after_open_without_losing_records` confirms v1 backup compatibility, while restore corruption, foreign-key and schema-shape tests continue to exercise the shared audit.
- Initial source SHA `6275a170da1d17922bbebfd864de524617d4ce95` failed the v1 backup compatibility regression: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40962191242 . Corrected source SHA `dff1a04f6cf9d20468c4f7ee883880a4aebf7209` passed:
  - Rust workspace (format, strict Clippy, tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/40962505838
  - Android Rust ABI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40962505862
  - Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40962505865 (running at documentation)
  - Android record codec probe: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40962505875 (running at documentation, unrelated to personal-state backup)

## Limits and next work

This does not guarantee that a mutable source file's filesystem identity cannot be swapped by another actor, provide a durable cross-process privacy-reset generation fence, enforce all target-OS owner-private directory/ACL requirements, or establish crash/power-loss and real desktop/Android restart acceptance. `STATE-001` remains unchecked until its complete criteria are evidenced.
