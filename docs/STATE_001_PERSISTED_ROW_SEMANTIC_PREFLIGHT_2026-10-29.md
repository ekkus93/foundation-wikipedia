# STATE-001 — Persisted row semantic preflight (2026-10-29)

## Problem and scope

SQLite structural checks, `quick_check` and `integrity_check` do not establish the application-level validity of every stored value. Previously, startup/backup-restore preflight checked bookmark and reading-position numeric CHECK constraints, but a forged history visit, chat role, pack digest, job status or setting key could survive and only fail if a specific bounded list/read later touched it. In particular, list limits could conceal a malformed row.

## Implementation

At exact implementation SHA `baaf9671608c38e2b00ef6b1743af9818921f582`, `wiki-store::user_state::validate_persisted_checks` now:

- Checks positive history row/page/revision/timestamp and chat-thread row/page/revision identities.
- Checks chat-message row/thread IDs, allowed `user`/`assistant` roles, nonempty content, 65,536-byte content cap and embedded NUL exclusion.
- Checks saved bookmark anchor's 4,096-byte limit and NUL exclusion.
- Streams all article identities in bookmarks, reading positions, history and chat threads through the canonical `ArticleKey` validator; no `LIMIT 1000` shortcut.
- Streams pack-owner IDs/digests, builder-job IDs/statuses and settings key/value pairs against the same conservative write-time contracts.
- Runs before existing v1 migration, on current-schema open, and before creating a backup-restore destination. Corrupt input is rejected without publishing a restored database.

This adds no schema version or migration and does not alter immutable article/pack storage.

## Regression and exact-commit CI

A disk-backed regression exercises invalid history timestamps/projects, chat revision/role/content, malformed pack digest, unknown builder status, invalid setting key and oversized bookmark anchor. Each test creates a fresh state database, mutates rows directly, then requires open rejection and backup-restore rejection with no destination publication.

- [Rust workspace — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40943992696)
- [Platform shells — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40943992952)
- [Android Rust ABI qualification — passed](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40943992796)
- [Android record codec probe — in progress at observation](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40943992775)

## Remaining acceptance

This is a local semantic consistency preflight, not a complete defense against concurrent SQLite writers, malicious file replacement after preflight, cross-process privacy-reset races, unsupported OS ACLs, or physical-device acceptance. Scanning all mutable rows also merits a realistic large-user-state startup performance measurement. `STATE-001` stays unchecked.
