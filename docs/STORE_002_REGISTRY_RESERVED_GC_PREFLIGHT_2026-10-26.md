# STORE-002 — SQLite-reserved GC inventory audit (2026-10-26)

Status: **partial implementation; STORE-002 remains unchecked**.

Added `MediaRegistry::with_reserved_unowned_media`: an IMMEDIATE SQLite
write reservation is acquired **before** enumerating unowned registered
media, and kept through a bounded caller-supplied inspection callback.
The reservation prevents competing SQLite writer connections from attaching
new owners during that callback. Validates digest and byte bounds, caps
candidates at 10,000 and rejects overflow without returning a partial
inventory. Transactions release the reservation even when an inspection
callback returns an error. No data or metadata is deleted.

Added `media_gc::plan_unowned_media_gc_with_registry_reservation` using
the same streaming SHA-256, byte-size and aggregate overflow validation as
the existing advisory GC planner. Tests exercise a real on-disk SQLite
database with a second connection attempting an owner insert while the
writer reservation is held, read visibility, release after inspection,
invalid bounds, corruption and preservation of metadata after errors.

**Critical limitation:** this is *registry writer coordination*, not
safe filesystem garbage collection. Installers currently publish media
bytes before ownership registration; readers do not yet participate in a
lease, and files may be modified or opened independently of SQLite.
Neither this API nor the existing GC plans authorize unlinking published
objects. Before deletion, implement a cross-process lease protocol
covering installations, file readers and GC, crash-safe tombstones/recovery,
and failure-injection tests. Do not mark STORE-002 complete based on this
limited reservation.

## Exact-head CI qualification

The first implementation SHA `899c00471f4afe4786b2c6f95af90f266a03499d`
failed the Rust formatting gate
([40458185645](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40458185645)).
The corrected exact code SHA `c6d9378575c473fa020e3311d4d900e357ddb585`
passed [Rust workspace CI 40458777426](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40458777426),
[Platform shells CI 40458777114](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40458777114),
and [Android Rust ABI qualification 40458777011](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40458777011).
[Android record codec probe 40458777030](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40458777030)
was not yet terminal when recorded; recheck before treating it as passed.
