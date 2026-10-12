# Provisional streamed .wpack container (PACK-001 partial)

**Status:** Rust format/verify API qualified on synthetic fixtures; not an installable or publisher-signed pack. Do not distribute as an official pack.

## Framing contract

`wiki_pack_format::container` uses eight-byte ASCII magic `FWPACK01`, followed by a big-endian `u32` manifest transcript byte count, then canonical `Manifest::identity_transcript()` bytes. Object bytes follow contiguously in ascending case-sensitive manifest path order; exact lengths are declared in the transcript. No path is extracted, interpreted or opened by the verifier.

The provisional transcript contains the v1 identity prefix, version, pack/project/snapshot, tagged origin, object count, and each object's path, digest and byte count. Length-prefixed strings use big-endian `u64` byte counts. `Manifest::validate` rejects unsafe/aliased paths, invalid origin and excessive declared resources. The parser additionally rejects alternate object ordering, noncanonical digest case, unrecognized tags and trailing bytes.

The manifest transcript is limited to 64 MiB in memory. Object verification streams 64-KiB chunks, checks SHA-256 and exact lengths without retaining entire objects. Writer callbacks stream into a length-enforcing, digesting writer; **the caller must use private temporary output and discard it if the writer returns an error**. The read-only verifier does not extract or activate content.

## Qualification

At exact code SHA `19dd8c17d4ca8c7e9b655b590fac75f4266a220d`, `cargo fmt --all -- --check`, strict workspace Clippy and workspace tests passed in [Rust workspace CI 38190698271](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38190698271). [Platform shells CI 38190698280](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38190698280) and [Android Rust ABI CI 38190698305](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38190698305) also passed. Tests cover canonical output under reordered manifests, digest casing, truncated/modified/appended payloads, oversized manifest header and short/long/wrong-content producers. These are host unit tests, not Android installation or airplane-mode acceptance.

## Security and product boundaries

This is an **unsigned transport prototype**, not publisher verification. Even an all-hashes-valid envelope can contain hostile HTML/SVG, missing required images, absent licensing, incompatible indexes or fabricated provenance. Publisher keys, source authentication, media discovery/completeness, safe staged extraction, atomic activation/rollback, quota checks on actual devices, per-object attribution, source snapshot/definition hashes and independent Android installation remain mandatory before checking PACK-001 or TRUST-003. The identity transcript is not a final signature scope; the full manifest must be designed before signing.
