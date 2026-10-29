# STATE-001 — Bounded persisted TEXT preflight (2026-10-29)

## Problem

The previous full semantic row audit correctly refused malformed persisted metadata but could deserialize an arbitrarily large forged SQLite TEXT value into a Rust `String` before rejecting its length. An attacker-controlled local database or backup could therefore amplify startup or restore memory use. SQLite can compute `length(CAST(column AS BLOB))` without returning the entire TEXT value to Rust.

## Implementation

Exact corrected code SHA: `f2be694b1dc1d08caa1f72a357d20c7be5b5071a`.

- Before streaming persisted rows, reject article project identifiers longer than 255 UTF-8 bytes across bookmarks, reading positions, history and chat threads.
- Reject pack-owner IDs over 128 bytes and digests over 64 bytes.
- Reject builder-job IDs over 128 bytes and statuses over 16 bytes.
- Reject settings keys over 128 bytes and values over 4,096 bytes.
- Retain prior byte-bound preflight for chat content and bookmark anchors; semantic checks still validate canonical syntax and exact allowed values after the bounded scan.
- No schema change or migration; the same audit runs on startup, v1 migration preflight and backup restore before destination creation.

## Regression

The disk-backed forged-state regression adds oversized project, pack ID and settings-value rows to the earlier nine invalid semantic-row cases. Every case must reject startup and backup restore without publishing a destination.

## Exact-commit CI

- [Initial source: Rust formatting failed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40944801092).
- [Corrected Rust workspace: passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40944912536).
- [Corrected Android Rust ABI: passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40944912621).
- [Platform shells: running at documentation](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40944912454).
- [Android codec probe: attempt 1 failed on corrupt NDK download; rerun attempt 2 pending](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40944912522).

## Limitations

The check is local semantic preflight, not proof against concurrent database replacement, cross-process writers, OS ACL misconfiguration, maliciously modified SQLite binaries or realistic large-database startup latency. `STATE-001` remains unchecked.
