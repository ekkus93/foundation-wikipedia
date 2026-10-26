# STORE-002: Registered-media attribution integrity (2026-10-26)

**Status:** Scoped read-only integrity hardening; STORE-002 remains unchecked.

## Change

A content-addressed media digest can be correct while its persisted attribution metadata has been lost, malformed, or polluted by excessive records. Offline media must retain creator, license, source and MIME notices; a successful byte-only audit must not be mistaken for an attribution-complete object.

Added `MediaRegistry::audit_registered_notices(digest, max_notices)` for registered objects regardless of pack/cache/pin ownership. It validates digest syntax, a 1–1,024 notice limit, every returned notice through the existing metadata validator, and the presence of at least one notice. The SQLite query reads at most `max_notices + 1` rows, so excessive metadata fails closed instead of being fully materialized. `media_gc::audit_registered_media_integrity` now calls this validator before rehashing each registered object.

## Evidence

Synthetic file-backed SQLite tests register a pack-owned figure, then mutate its attribution outside the normal API: an embedded newline in the creator is rejected, removing the last notice is rejected, adding 1,024 extra notices above the default bound is rejected, and restoring a single valid notice allows the audit to succeed. The tests retain byte integrity throughout to prove metadata failure is independently detected.

Initial source SHA `dbcc139873f3dbb3372e716289f5abcf9a852d63` failed only rustfmt ([Rust workspace 40405510435](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40405510435)). Corrected exact code SHA `7173e0e111336c635380bdc86c6e36b2aded7bcc` passed [Rust workspace 40406083625](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40406083625), [Platform shells 40406083109](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40406083109) and [Android Rust ABI 40406083166](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40406083166). [Android record codec probe 40406083403](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40406083403) was in progress at last observation.

## Limitations and next work

This is a nontransactional read-only audit of caller-managed registry rows and local content-addressed bytes. It does not authenticate the attribution's publisher, validate the media's actual MIME against its bytes, prove required visual coverage, authorize deletion, coordinate readers and installers, or handle GC crash recovery. STORE-002 and the broader offline acceptance checklist remain unchecked. A production collector needs a shared reader/writer coordination protocol and a durable, recovery-safe deletion state machine.
