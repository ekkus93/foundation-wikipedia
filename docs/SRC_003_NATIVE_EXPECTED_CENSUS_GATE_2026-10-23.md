# SRC-003 — Native bzip2/XML census with independent expected-page gate

**Date:** 2026-10-23  
**Status:** Qualified partial implementation; SRC-003 remains unchecked.  
**Exact qualified code SHA:** `243735ac0d741cec4d7fa3e11098376b85d7112d`

## Implementation

`wiki-source::public_dump_census_gate::stage_and_audit_native_bzip2_census` composes the native, inventory-pinned multistream bzip2/XML page decoder with the existing bounded expected-versus-observed census gate. `NativeCensusAudit` explicitly binds the expected census path and SHA-256 digest with compressed/decoded byte budgets. The decoder obtains each compressed member digest from the pinned inventory, not the stream provider.

All observed IDs remain provisional in a private staging file. The requested destination is created via a no-overwrite hard link only after every shard, checksum, XML/page event, namespace-specific census export, expected-file digest and exact page-ID coverage comparison succeeds. Failed reads, malformed compressed data, expected-page mismatches, and preexisting destinations leave the prior publication untouched and remove temporary outputs.

## Automated qualification

- Qualified source: `243735ac0d741cec4d7fa3e11098376b85d7112d`.
- Rust workspace (format, strict Clippy, tests): https://github.com/ekkus93/foundation-wikipedia/actions/runs/39988375051 — **passed**.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/39988375024 — **passed**.
- Native regression compresses two synthetic XML shards with bzip2 and verifies exact `1\n201\n` output, contributor-ID exclusion, no-overwrite, correct-digest but mismatched expected page IDs, forged second-member digest and cleanup without publication.
- Intermediate commits `91db18e91331c001dabda0f6f1e357b617a3eb49`, `a452fd29b0d0eb5552ae0fe361c5ccccfa02106c`, and `856852d1b13a9d82b6de300184c6119c64c4ad4d` failed formatting or strict Clippy and were repaired before qualification.

## Remaining gates

A SHA-256 digest pin **does not authenticate Wikimedia as publisher** and does not prove the expected census was independently derived. Callers must authenticate both publications, verify snapshot cutoff and namespace semantics, and prove that the expected-page list is complete. This narrow XML adapter is not a fully conforming MediaWiki parser; rendering fidelity, mandatory offline images/diagrams/math/attribution, pack activation and real desktop/Android acceptance are not qualified. **Do not mark SRC-003 complete or activate a snapshot from this evidence alone.**
