# STORE-004 — Published read-only FTS5 native integrity verification (2026-10-28)

## Change

`OfflineSearchIndex::open_verified_read_only` now runs the existing full catalog and redirect/body-byte membership audit and then a native SQLite FTS5 integrity check on a private writable **consistent clone** of the published index. SQLite `VACUUM INTO` copies the read-only source (including the committed WAL snapshot) into an exclusively created mode-0600 scratch file in the privately owned index directory. FTS5's native `integrity-check` runs against that clone; the source database is never opened for writing by the verifier. Scratch files are deleted on both success and failure. An error, including insufficient temporary disk space, fails closed.

## Regression

The disk-backed verified-open test checks unchanged published bytes after successful opening, corrupts the FTS5 `docsize` shadow table while leaving stored article text and normal search valid, proves catalog membership alone still passes, proves verified opening rejects the corrupted postings, confirms no audit scratch files remain, and rebuilds the derived index from verified catalog frames to restore verified opening.

## Operational and security constraints

- The caller must own a private, stable directory with no concurrent replacement; path preflight is not a defense against a malicious local writer racing a pathname.
- Each verified open now requires an additional temporary database roughly the size of the FTS index and a complete clone/read pass. This is a correctness-first activation gate, **not** a per-search operation. Disk exhaustion or SQLite cloning failure denies verified opening rather than weakening checks.
- Read-only `open_read_only` remains deliberately unverified. Product adapters must explicitly use the verified path and separately authenticate their source snapshot.
- This does not authenticate Wikimedia publisher identity, ensure all source/media content was acquired, or qualify desktop/Android installed-pack activation. STORE-004 remains unchecked.

## Verification

Source and tests committed to `master` using Ralph Bridge exact-head compare-and-swap; consult the exact-head Rust workspace, Platform shells and Android CI for the resulting commit. Do not infer their outcomes before observing them.
