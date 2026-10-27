# STORE-002 — Local SQLite and media-root write-permission guards

Date: 2026-10-27  
Status: **partial qualification — STORE-002 remains unchecked**

## Change

The media lease is advisory: it excludes only cooperating processes. A group/world-writable SQLite registry allows another local principal to alter media owners or the persistent root-binding record without participating in the lease protocol. On Unix, `MediaRegistry::prepare_registry_path` now rejects an **existing** SQLite ownership database if its permission bits include group or other write (`mode & 0o022 != 0`), as well as the existing hard-link/symlink checks. Both `open` and `open_existing` share this preflight. New SQLite files continue to be created with mode `0600`; an operator can restore a mistakenly permissive registry to owner-only mode before retrying.

For the CLI's destructive `media-maintenance --confirm-delete` and `--bind-registry` modes, the media-root directory itself must also not have group/world write permissions. Read-only `--dry-run` and `--audit-unbound` remain available for diagnostics. This guard prevents an obvious unsafe local configuration from being silently accepted as deletion authority.

Unix regressions chmod the SQLite registry to `0620`, `0602` and `0666` and prove both registry open paths reject it; restoration to `0600` succeeds. CLI regressions chmod the media root to `0777`, reject both destructive modes while verifying pinned and unowned objects remain readable, then restore `0700` and continue normal cleanup.

## Exact-head CI

- Registry permission guard SHA `348302716b38d93ca7707296fe962a1ff5b06f6f`: [Rust workspace 40538943701](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538943701), [Platform shells 40538943653](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538943653), and [Android Rust ABI 40538943669](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40538943669) passed.
- Initial CLI guard SHA `ac5a9ae48f914b806c660544ffd563599f8204ef` failed rustfmt ([Rust 40539170773](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40539170773)); corrected SHA `3486989bf360c40d667ceb6893f126c47df811e5` passed [Rust workspace 40539284785](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40539284785); [Platform shells 40539284916](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40539284916) was running when documented.

## Security boundary

These are configuration preflight checks, **not** full protection against hostile filesystem activity. An attacker with the same UID or direct SQLite access, a mutable ancestor directory, a permission change after validation, or a swapped inode can bypass the advisory coordination. Neither this change nor the root-binding mechanism authenticates a publisher, licenses, pack contents, or the complete offline visual inventory. Scheduled maintenance, hostile directory-race mitigation, disk-full and physical power-loss testing, production pack lifecycle wiring, and desktop/Android device acceptance remain.

**STORE-002 remains unchecked.**
