# STORE-002 — Bounded owner-scoped attribution reads (2026-10-26)

Reader-facing `MediaRegistry::owned_digest_notices` now uses a bounded SQLite query (`LIMIT 1025`) and rejects a 1,025th attribution row instead of accumulating unbounded untrusted notice metadata. It also rejects multiple MIME declarations for identical content. `owned_digest_mime` validates the persisted MIME syntax before exposing it to a renderer, including after SQLite tampering.

Regression tests exercise conflicting MIME values, 1,025 notices for one object, missing owner scope for another pack, and an invalid MIME value written directly to SQLite. Normal multi-creator attribution and owner revocation behavior remain supported.

**Qualification:** Exact-head CI must be checked after the implementation commit. These checks do not authenticate publisher notices or source completeness, do not discover missing visual dependencies, do not implement safe deletion/GC, and do not establish device acceptance. STORE-002 remains unchecked.
