# STATE-001 — Guarded SQLite backup restore (2026-10-29)

## Scope

`wiki-store::user_state::UserState::restore_backup_to_new_path` restores a user-state SQLite backup into a **new** user-state file. It does not replace an open or existing database or activate a Wikipedia pack. The caller must choose a trusted private directory and coordinate writers; it must not source a concurrently modified backup.

Before destination creation, the source is opened read-only, checked with `PRAGMA integrity_check` and `pragma_foreign_key_check`, validated as user-state schema v1 or v2 with all required tables, and checked to reject symlink/non-file backup paths (plus hard links on Unix). A malformed or unsupported input cannot overwrite a working user's state.

SQLite `VACUUM INTO` writes a consistent standalone copy into an exclusively created destination (Unix mode `0600`). The destination is synced and rechecked for structural integrity; failure removes the new candidate on a best-effort basis. Schema-v1 data is preserved and migrated only by subsequent `UserState::open`.

## Regressions

- Reopen restored state and verify bookmarks, reading position, opt-in history, media ownership, and builder-job metadata.
- Verify backups and restored state are independent and the original backup bytes are unchanged.
- Refuse overwrite on repeat restore.
- Refuse blank, future-version, incomplete and non-SQLite files without publishing a restored database.
- Refuse Unix symlinked backup input and require `0600` restored permissions.
- Restore an existing schema-v1 backup and verify normal v2 migration, original bookmark and object ownership, and the migrated ownership index.
- Reject an otherwise valid SQLite backup with an orphaned chat-message foreign key before creating the destination.

## Limitations

This is not an installed restore UI, replacement transaction for an active SQLite database, full schema-semantic validation, cross-process lease, Windows ACL qualification, device test or completed STATE-001 task. The backup and user-state private directory must be protected from concurrent untrusted modification.

## Exact-head CI

Initial restore code commit `7f91cbc86a7291af915c9c82a8aae8414066724b` failed formatting; corrected implementation `b53c79a8e8b91831756c692845e0265c874db64e` passed [Rust workspace 40859118169](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40859118169) and [Android Rust ABI 40859118144](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40859118144). Platform shells and codec probe were pending at this documentation update. The additional v1 migration/foreign-key regressions have not been qualified until their exact resulting commit passes CI.
