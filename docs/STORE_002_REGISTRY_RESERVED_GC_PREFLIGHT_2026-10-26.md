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
