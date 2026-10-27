# STORE-002 — Durable media staging cleanup and alias safety

**Date:** 2026-10-26  
**Status:** Partial STORE-002 implementation; not a production garbage collector.

## Problem and implementation

The content-addressed media writer publishes a verified object using a no-overwrite hard link from a private temporary staging file. Until the temporary name is removed, the inode has two links. The reader intentionally refuses persistent hard-linked objects because an outside alias can mutate their bytes. Previously the writer synced the directory *before* removing the temporary name but did not check or sync the removal before reporting success. A crash could therefore leave an object with an extra link that the reader rejects.

- After successful publication, remove the staging name and sync the containing directory before returning the digest. A cleanup or sync error is now surfaced rather than silently reporting success. On publication failure, preserve published objects and attempt best-effort temporary cleanup.
- Reject lexical `..` components in the media root before resolving object paths, including during object enumeration. A path that currently resolves inside the store is still unsafe if a directory is later replaced.
- On Unix, reject hard-linked files masquerading as ignored staging files during the bounded published-object scan. The scanner never follows or deletes such entries.

The existing no-overwrite publication and bounded verification behavior remain intact. These are durability and alias-preflight improvements, not cross-process GC coordination or a guarantee against malicious concurrent directory replacement.

## Tests and CI

New Rust regressions in `crates/wiki-store/src/media_objects.rs`:

- `successful_promotion_removes_staging_hardlink_before_returning`: verifies a single published object name and inode link after initial and idempotent writes.
- `lexical_parent_traversal_is_rejected_for_reads_writes_and_scans`: rejects lexical parent traversal while a legitimate store remains readable.
- `published_scan_rejects_hardlinked_staging_aliases`: rejects a hard-linked ignored staging file, preserves external bytes and succeeds after removing the alias.

Implementation SHA: `cd0bf6b79f27110b3f0125d346d45d8033469e95`.

- [Rust workspace CI 40456336518](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40456336518) — passed.
- [Platform shells CI 40456336629](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40456336629) — passed.
- [Android Rust ABI CI 40456336569](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40456336569) — passed.
- [Android record codec probe 40456336584](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40456336584) — in progress at documentation.

## Remaining requirements

No filesystem deletion API has been introduced. Production STORE-002 GC still requires a shared cross-process coordination protocol covering active readers, installers and the SQLite ownership registry; atomic retirement and crash recovery; safe handling of unregistered objects; and real fault-injection and device qualification. The fsync ordering is not itself a simulated power-loss test. Publisher authentication, complete required offline media and pack activation remain separate requirements. **STORE-002 remains unchecked.**
