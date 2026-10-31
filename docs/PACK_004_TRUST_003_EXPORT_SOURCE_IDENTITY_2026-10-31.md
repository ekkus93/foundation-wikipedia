# PACK-004 / TRUST-003 — Custom export source identity guards

**Date:** 2026-10-31  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `820535d8de440a136c3d51c51e2ef19443bc02d4`

## Problem and scope

The provisional unsigned custom pack writer already checked every object's SHA-256 and length, but its initial file inventory and final streaming producer did not consistently compare the source-file identity and change timestamps *after* reading. A privately owned source tree may be accidentally replaced during export with a different inode containing identical bytes: the written envelope remains digest-valid, but the builder can no longer attest that the exact originally inventoried file remained stable throughout the operation. This improvement rejects the detectable mutation rather than silently accepting it.

## Changes

- `crates/wiki-pack-format/src/objects.rs`: share the existing `same_file_snapshot` check with the custom exporter within the crate. On Unix it requires the same device/inode, unique link, length, and modification/change timestamps; other platforms retain the existing best-available modification/creation metadata comparison.
- `crates/wiki-pack-format/src/export.rs`: compare path-to-open-descriptor identity during inventory, then opened-descriptor-to-final-descriptor and final-directory-entry identity after the complete object hash. During envelope streaming, perform the analogous checks before and after `io::copy`. Fail closed on detectable change; the output buffer may contain a partial envelope on failure and must never be published until the caller's final verification/durability checks pass.
- Deterministic unit regressions simulate a post-hash same-digest inode replacement and a writer-triggered source replacement after emitting object bytes. These tests do not rely on sleep/timing a racing thread. Existing normal canonical export and hostile-input tests remain in the Rust workspace suite.

## Qualification

- Initial `f70844108d777d3528c28cc97883c953559bb808`: [Rust workspace 41223643969](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41223643969) failed rustfmt on an assertion layout.
- **Corrected `820535d8de440a136c3d51c51e2ef19443bc02d4`:** [Rust workspace 41223713429](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41223713429) **passed**; [Platform shells 41223713333](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41223713333) **passed**; [Android Rust ABI qualification 41223713288](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41223713288) **passed**.

## Release boundary and remaining work

Filesystem metadata checks are defense in depth, **not** an atomic snapshot, a locking scheme, or a defense against attackers who control the staging tree while it is being read. Exclusive ownership of immutable input content is still required. This work does not authenticate a publisher, establish full offline media and license completeness, implement install/rollback, or qualify real Android and desktop devices. **Do not mark PACK-004 or TRUST-003 complete on this evidence.**
