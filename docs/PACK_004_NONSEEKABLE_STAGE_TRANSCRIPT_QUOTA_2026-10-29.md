# PACK-004 / TRUST-003 — Nonseekable staging transcript quota preflight (2026-10-29)

## Scope

The unsigned `.wpack` nonseekable staging entrypoint `stage_container_stream_with_budget` previously wrapped `verify_container` in a disk-quota tee. The tee prevented spool bytes from exceeding the quota, but the generic verifier could allocate up to 64 MiB for a declared manifest transcript before reading those bytes, even when the caller had allowed only a tiny spool. This was a resource-budget inconsistency for hostile document-provider or stdin streams.

The staging path now calls the existing `verify_container_with_budget` over the spooling tee. Its 12-byte header preflight rejects a claimed transcript that cannot fit within the caller's complete spool budget before the transcript allocation. The inner bounded reader preserves object-digest, canonical-transcript, trailing-byte and total-byte enforcement; the outer tee continues to limit persisted bytes and removes the spool on failure. Seekable staging and the no-quota entrypoint retain their previous behavior.

## Synthetic regression

`nonseekable_spool_preflights_manifest_allocation_against_quota` supplies a forged `FWPACK01` header claiming a 64-MiB transcript with a 32-byte spool budget. The importer returns `ResourceBudget` after exactly 12 input bytes, with no staged directory or spool left in the private parent. An 11-byte budget rejects without reading any source bytes. Existing nonseekable exact-quota success, one-byte-overflow cleanup, digest-corruption and provider-disconnect tests remain in the Rust workspace suite.

## Exact-head qualification

- Initial source SHA `818249220b9facce9ba3dd4811aff6a9edce961d` failed Rust formatting only: [Rust workspace 40885185032](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40885185032).
- Corrected source SHA **`6311f45e28d295510650280ea8a77d3df315068e`** passed [Rust workspace 40885367706](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40885367706), [Platform shells 40885367617](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40885367617), and [Android Rust ABI qualification 40885367631](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40885367631).

## Boundaries

This is a synthetic host-side memory-budget hardening slice. It does **not** authenticate a publisher, establish mandatory offline image/diagram/math and attribution completeness, install or activate a pack, integrate an Android SAF picker, or qualify physical device/power-loss recovery. PACK-004 and TRUST-003 remain unchecked.
