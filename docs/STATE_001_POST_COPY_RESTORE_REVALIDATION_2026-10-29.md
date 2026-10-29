# STATE-001 — Post-copy SQLite restore revalidation (2026-10-29)

## Threat and change

The guarded restore previously validated an untrusted SQLite backup before copying it, but the final check of the `VACUUM INTO` output covered only SQLite structural integrity. A separate writer could commit a schema or persisted-row change between source preflight and the independently timed copy. SQLite `integrity_check` can still return `ok` for semantically invalid values such as an unrecognized builder-job status.

The restore now requires the copied database to retain the same supported schema version as the vetted source, pass the complete schema/table/index audit, have no foreign-key violations, and satisfy the same bounded persisted-row semantic checks. The destination is deleted on failure. This is a second verification boundary, not a claim that an untrusted source is frozen during the operation.

## Deterministic regression

`restore_rechecks_copied_rows_after_concurrent_source_mutation` opens a valid disk-backed state database and passes the source preflight. A separate SQLite connection then commits an invalid builder-job status before the restore's `VACUUM INTO`. The resulting copy must fail semantic verification and the destination path must not remain. The test repairs the source and verifies a subsequent ordinary restore preserves the valid job. The production API uses an empty preflight hook; the private helper supplies a deterministic injection point for this test.

## Exact-commit evidence

- Initial source `7f16588b383ff2db17c9433c6e89970b4a6989bf`: [Rust workspace formatting failure](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40947497094).
- Corrected source `23fa86c439e4ebec44cf57c7b7a40e3a6991a0b3`: [Rust workspace passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40947856462), [Platform shells passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40947855693), [Android Rust ABI passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40947855748).
- [Android record codec probe](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40947856034) was in progress when this qualification was recorded.

## Remaining acceptance

This closes a semantic post-copy validation gap only. It does not secure backup path selection against OS-level source replacement, guarantee cross-process locking, qualify Windows ACLs, benchmark large state databases, or prove physical-device/restart acceptance. `STATE-001` remains unchecked.
