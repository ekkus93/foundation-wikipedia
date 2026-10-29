# PACK-001 / TRUST-003 — Stage Occupancy and Durability Rollback (2026-10-29)

## Scope

The provisional unsigned .wpack stagers now preflight an occupied destination under the caller-owned private parent before consuming any archive bytes, whether the source is seekable or an Android-style nonseekable stream. An existing file, directory or symlink at the destination is rejected with AlreadyExists. The existing exclusive create remains authoritative against races between preflight and staging. An archive is never permitted to overwrite an existing pack.

The two-pass stager also removes its newly created staged tree if the parent directory fsync fails after all object checks, rather than reporting failure while leaving a seemingly installable tree behind. Cleanup itself may still fail due to OS or storage errors; such a failure propagates as an error. This is staging, not installation, activation or publisher authentication.

## Synthetic regression and exact-commit CI

- An input that panics on any Read or Seek proves both seekable and stream APIs reject an occupied target before input access. A sentinel file remains unchanged and no spool is left.
- A deliberately injected parent-sync I/O failure proves the finalization helper removes a newly created stage tree.
- Code SHA: `45b4b4922ae3bc6d7d60df17f5fdcf3884bea670`.
- Rust workspace (fmt, Clippy, tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/40872962705 — success.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40872962135 — success.
- Android Rust ABI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/40872962143 — success.
- Two intermediate source commits (`19bf72c26b34235903a5ae7077630f93e391f90c`, `8fb66470e05183bb9cdf1675d98ae65abed25fc9`) failed rustfmt and were corrected before this qualification.

## Outstanding

No production Android SAF call site, device tests, authenticated publisher signature, mandatory offline media completeness, atomic pack activation, crash/power-loss qualification, descriptor-relative race resistance, or owner quota/free-space enforcement is demonstrated. Both PACK-001 and TRUST-003 remain unchecked.
