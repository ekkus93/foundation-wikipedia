# STORE-002 — CLI binding dispatch and opt-in scheduled media GC

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Actual operator CLI bug fixed

The library already implemented `--audit-unbound` and `--bind-registry`, but the `wiki-pack` binary dispatcher only admitted `--dry-run` and `--confirm-delete`. These modes were therefore unreachable from the documented executable even though the internal module unit tests passed. The dispatcher now admits all four explicit modes. New black-box integration tests run `CARGO_BIN_EXE_wiki-pack` and prove unbound auditing, guarded binding, orphan refusal, dry-run preservation, explicit deletion and saved-user-pin retention.

## Opt-in timer-ready bounded maintenance

The new `--run-if-due` command is a **destructive** mode explicitly selected by the operator. It is designed for periodic invocation from cron, systemd or an application job; it does **not** create or enable a background service on its own.

```sh
wiki-pack media-maintenance --audit-unbound /private/wiki/media /private/wiki/owners.sqlite 1000 1000
# After manually reconciling all orphans and verifying this is the correct registry:
wiki-pack media-maintenance --bind-registry /private/wiki/media /private/wiki/owners.sqlite 1000 1000
wiki-pack media-maintenance --dry-run /private/wiki/media /private/wiki/owners.sqlite 1000 1000
# Run from an operator-installed timer, e.g. hourly, with a 24-hour success interval:
wiki-pack media-maintenance --run-if-due /private/wiki/media /private/wiki/owners.sqlite 1000 1000 86400
```

Both candidate budgets must be 1–10,000; interval must be 60–2,592,000 seconds (one minute to thirty days). An existing, initialized registry **outside** the media root and a correct persistent root binding are mandatory. On Unix the destructive path rejects group/world-writable media roots; the registry opener rejects group/world-writable or multiply hardlinked SQLite database files. The first scheduled invocation runs immediately if no prior successful pass is recorded.

Under a single EXCLUSIVE cross-process media lease, the API validates the root binding, reads the durable success timestamp, skips when not due, runs both bounded registered-unowned and unregistered-orphan cleanup phases, and writes the success watermark to the same SQLite ownership registry **only when both phases succeed**. A partial post-commit unlink failure remains explicitly reported and is eligible for retry on the next invocation; it cannot silently mark the run successful. If checkpoint persistence fails after successful cleanup, the CLI reports an error rather than falsely claiming a durable success. A clock rollback never makes a run due early. The persisted watermark survives registry close/reopen; cooperating overlapping scheduled invocations serialize on the lease.

This is **not** an OS timer installation, a guarantee that future invocations occur, or a defense against malicious direct SQLite/filesystem writes. Operator must install and monitor the external timer and ensure the directory is not writable by untrusted users.

## Evidence

- CLI dispatch correction `8a88da8bfe61906afeb99e3265ad9afd306d87cf`: [Rust CI 40543562169](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40543562169), [Platform CI 40543562176](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40543562176) **passed**.
- First scheduled source `3314952fe55d3e4e91476cb89b474a5d8c4eb4c7` failed rustfmt [40544391496](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40544391496). Formatted source `581cc413e882aac24a7332ee8234555416e1582d` failed strict Clippy's large enum variant [40544845715](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40544845715).
- Corrected scheduler source `dcf56110a46fb94563dfd48f3c51348515931a31` passed [Rust 40545084857](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545084857), [Platform 40545084896](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545084896), [Android Rust ABI 40545085390](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545085390). [Android record codec probe 40545085034](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545085034) was running at last source check.
- The scheduled CLI regression first failed rustfmt at `3b6e8532f8997cd9fed69c815c24027e4b80d955` [Rust 40545410750](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545410750). Corrected `384bba29c3526a9bb44df77e6d2f145f767dff0c` passed [Rust workspace 40545509682](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545509682) and [Platform shells 40545509886](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40545509886).

## Remaining acceptance gates

- Install and qualify an actual OS/app scheduling integration with retries, cancellation, observability and lifecycle ownership, rather than only a safe opt-in due gate.
- Wire production pack/cache/pin uninstall and snapshot transitions to the bound media APIs; validate on real desktop and Android storage.
- Exercise disk-full, process kill, fsync and physical power-loss fault injection; harden hostile same-UID/uncooperative SQLite and directory mutation.
- Complete official publisher provenance, required offline visuals and end-to-end installed-pack/device acceptance.

**STORE-002 remains unchecked.**
