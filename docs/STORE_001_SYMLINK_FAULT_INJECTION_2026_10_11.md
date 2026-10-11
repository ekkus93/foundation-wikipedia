# STORE-001 — Persisted shard symlink fault injection (partial)

Implementation `77e9c1fa56fada92573af18242b246746ce3559d` adds a Unix regression to `crates/wiki-store/tests/catalog_persistence.rs`: after catalog persistence/reopen, corruption detection and missing-shard handling, a symlink is substituted for the shard pathname. `SnapshotCatalog::read_record` must reject the substituted path with `InvalidFrame`, even when the symlink target contains otherwise valid frame bytes.

Exact-head qualification:
- Rust workspace [38114016817](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38114016817) — passed.
- Platform shells [38114016766](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38114016766) — passed.
- Android Rust ABI [38114016796](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38114016796) — passed.
- Android record codec probe [38114016781](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38114016781) — still in progress at evidence capture.

**STORE-001 remains unchecked:** full authenticated source-to-catalog coverage, large-snapshot/Android random-access runtime acceptance, atomic activation and broader crash/disk-full fault injection are not qualified. An attempted source change to bind streamed whole-shard hash verification to the opened inode was rejected by the tool safety gate; that improvement is not implemented by this test.
