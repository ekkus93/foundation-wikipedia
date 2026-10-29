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

## Limitations

This is not an installed restore UI, replacement transaction for an active SQLite database, full schema-semantic validation, cross-process lease, Windows ACL qualification, device test or completed STATE-001 task. The backup and user-state private directory must be protected from concurrent untrusted modification.

## Exact-head CI

Pending exact commit confirmation at source submission. Record outcomes through Ralph Bridge before qualification.
