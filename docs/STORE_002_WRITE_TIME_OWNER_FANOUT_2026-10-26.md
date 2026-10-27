# STORE-002: Write-time media owner fanout bounds (2026-10-26)

**Status:** Scoped implementation and regression qualification; STORE-002 remains unchecked.

## Defect and repair

The read-only media integrity audit refuses more than 1,024 owners for any digest, but the public registration APIs previously allowed an otherwise valid pack, cache or user-pin write to exceed that ceiling. Such writes could make an existing pack's otherwise intact offline media fail integrity verification.

`MediaRegistry::add_owner` now enforces the 1,024-owner limit at the write boundary. The SQLite `INSERT OR IGNORE ... SELECT ... WHERE COUNT(*) < 1024` guard checks capacity in the **same write statement** as the insertion, rather than using a racy application-level count-then-insert. Existing claims remain idempotent at exactly 1,024. Historical corrupt over-limit states are refused, and a new claim beyond the limit returns `TooManyOwners`.

`MediaRegistry::register_verified_owned_batch` checks owner capacity inside its existing IMMEDIATE transaction before each ownership insertion. An overflow aborts the entire batch: earlier new object rows, attribution notices and owner claims are rolled back together. Reinstalling an existing owner's exact inventory remains valid at the ceiling.

## Regression and compatibility evidence

Synthetic Rust tests cover the exact 1,024-owner boundary, idempotent existing owner, rejection of a 1,025th owner, removal followed by replacement, batch rollback of a newly registered digest and attribution when a later shared digest is full, and a successful exact-inventory reinstall by an existing owner. The integrity audit remains able to verify a valid full-capacity digest.

The cap is deliberately aligned with `audit_registered_owners`; it does not establish a pack-wide ownership limit or coordinate byte deletion. It does not prevent out-of-band SQLite modification, certify source attribution, authenticate a publisher or prove complete offline visual assets.

## Exact-head CI

- Initial source SHA `c67dee8497920176de811103801d8b6b2ebc206c` failed formatting: [Rust CI 40438982715](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40438982715).
- Corrected implementation SHA `f8070c4d24317a19f4abb373a92e82a602f985f5`.
- [Rust workspace CI 40439209059](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40439209059) — passed (rustfmt, strict Clippy, tests).
- [Platform shells CI 40439208995](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40439208995) — passed.
- [Android Rust ABI CI 40439209018](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40439209018) — passed.
- [Android record codec probe 40439209038](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40439209038) — running when documented.

**Still outstanding:** concurrency-safe, crash-recoverable media garbage collection coordinated with active readers and installers; publisher-authenticated provenance; full offline visual discovery; pack activation and real desktop/Android acceptance.
