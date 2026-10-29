# STATE-001 — Private SQLite backup qualification (2026-10-29)

## Scope and implementation

`wiki-store::user_state::UserState::backup_to_path` exports the currently committed mutable SQLite state as a standalone database through SQLite's `VACUUM main INTO ?1`. This is not a byte-for-byte copy of a live database or its WAL file. The export includes bookmarks, revision-scoped reading positions, opt-in history, chat state, nonsecret settings, pack ownership and job metadata. The method does **not** export canonical article snapshots or installed pack payloads.

The caller must provide a **new path in a trusted, private directory**. The method opens it with exclusive creation, rejecting existing files and symlinks instead of overwriting user data. Unix creation uses mode `0600`; on Windows, ACLs inherit from the chosen directory and are not yet independently qualified. After successful SQLite export, the destination file is synced. If the export fails, the incomplete destination is removed on a best-effort basis. The backup contains personal information and must not be put in a public pack, diagnostic bundle, or shared directory.

## Automated regression

`crates/wiki-store/src/user_state.rs::private_backup_is_consistent_and_refuses_to_overwrite_existing_state` creates a fresh private backup from an in-memory database, reopens the backup with the public API, and checks that bookmark, revision-scoped reading position, opted-in visit history and content-object ownership survive. It then modifies the source and verifies that a second backup to the same destination fails without changing the original backup bytes. On Unix, it checks mode `0600`.

Validation is performed on the **exact implementation commit** `8559bc460417a8c7074db5dd5533ba88d1047281`:

- [Rust workspace CI 40846206156](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40846206156) — passed (`cargo fmt --all -- --check`, strict Clippy, Rust workspace tests).
- [Platform shells CI 40846206065](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40846206065) — passed.
- [Android Rust ABI qualification 40846206064](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40846206064) — passed.

The first implementation commit `c1e7a72e9b61aee37569bdeeae1a1f2e18004a22` failed Rust formatting ([40846106839](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40846106839)); formatting was corrected in the qualified implementation commit.

## Remaining acceptance

This is a **backup primitive**, not a complete backup-and-restore product. There is no restore/import flow, OS-specific destination picker or verified Windows ACL policy, scheduled backup, cross-process state lock, or physical-device acceptance. No claim is made that backup restoration or WAL concurrency has been independently stress-tested. `STATE-001` remains unchecked; installed packs and immutable source snapshots remain separate from this user-state export.
