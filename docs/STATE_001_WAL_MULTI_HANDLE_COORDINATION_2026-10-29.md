# STATE-001 — Disk-backed SQLite WAL and bounded concurrent writer retry (2026-10-29)

## Scope and rationale

The mutable personal-state SQLite database is separate from immutable Wikipedia snapshots. Previously, disk-backed `UserState::open` used SQLite's default journal mode and default immediate lock-error behavior, so a short write transaction from another connection could fail an otherwise valid concurrent update. This slice enables WAL and a five-second SQLite busy timeout for disk-backed user state. The in-memory test database does not change journal mode.

## Implementation

- `crates/wiki-store/src/user_state.rs`: disk-backed `UserState::open` sets `Connection::busy_timeout(Duration::from_secs(5))` before schema initialization, then requires `PRAGMA journal_mode=WAL` after successful initialization. An unsupported journal-mode result is rejected.
- The disk-backed regression opens two independent handles, holds an uncommitted `BEGIN IMMEDIATE` writer transaction, proves that a second handle sees only committed values, and verifies a second writer waits and successfully commits after the first transaction completes.
- Existing backup/export still uses `VACUUM main INTO` for a consistent standalone SQLite image; it must not copy the live `-wal` file directly. The WAL and shared-memory sidecars belong in the same owner-private state directory.

## Exact-commit qualification

- First implementation SHA `4ca4fc67648df702ce4d90b8f62858b1141def0c`: [Rust workspace 40952704230](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40952704230) failed formatting only. This was corrected rather than ignored.
- Corrected source SHA `368a54a05e922945c51be5bbfe7715c2e9b4a2db`: [Rust workspace 40952858836](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40952858836), [Platform shells 40952858828](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40952858828), and [Android Rust ABI qualification 40952858841](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40952858841) all passed. [Android record codec probe 40952858826](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40952858826) was in progress when this document was written; it does not independently qualify personal-state access.
- Regression: `disk_backed_handles_use_wal_and_wait_for_concurrent_writer`, exercised by the Rust workspace test suite. No physical-device test was performed.

## Remaining limitations

WAL allows readers and one writer to coexist, but it is **not** a durable application-level cross-process coordination protocol. Concurrent schema migrations can still race because version discovery and migration are not fenced by one shared write transaction. Backup source replacement, OS-level ACL qualification, privacy-reset fencing across processes, startup stress/latency benchmarks, and desktop/Android restart acceptance remain open. Do not check STATE-001 until its complete acceptance criteria are evidenced.
