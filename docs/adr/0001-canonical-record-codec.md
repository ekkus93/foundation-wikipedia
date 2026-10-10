# ADR 0001 — Canonical article-record encoding

**Status:** Accepted  
**Date:** 2026-10-10  
**Requirements:** MOD-003, STORE-001, PACK-001, UPD-004

## Context

Foundation Wikipedia needs canonical article and redirect records that are compact, independently addressable, bounded on mobile, and explicit about schema compatibility. The logical data model remains in `wiki-model`; the physical storage encoding belongs to `wiki-store`.

The evaluated binary serialization candidates were named-field MessagePack (`rmp-serde`) and CBOR (`ciborium`). Both were measured on the same representative Unicode/structured-media/reference fixture, with and without record-level zstd level 3 compression. A framed shard probe also measures direct-offset record access without decompressing preceding records.

## Decision

Use **named-field MessagePack wrapped in per-record zstd level 3**, inside the versioned `FWREC001` record envelope.

Each encoded record carries:

- codec major/minor version,
- explicit payload-encoding and compression identifiers,
- reserved header bytes that must remain zero until assigned,
- declared bounded uncompressed and compressed lengths,
- one independently compressed MessagePack payload.

Shards are sequences of length-prefixed record envelopes. The future SQLite catalog can persist each frame offset and length, allowing one record to be located and decompressed without scanning or inflating the rest of the shard.

The current codec version is **1.1**. Version policy is fail-closed:

- current `1.1`: decode normally;
- legacy `1.0`: decode only after all normal validation, return `RewriteToCurrent`, and write back only as the current codec;
- future minor versions: reject explicitly;
- different major versions: reject explicitly.

This deliberately avoids speculative forward decoding of unknown fields or semantics at the storage-envelope layer.

## Measured evidence

Exact commit `f89f6a43b1153a044f4deba7bc269d19ee94e912`, Rust workspace CI run 38019218520:

| Measurement | MessagePack | CBOR |
| --- | ---: | ---: |
| Uncompressed fixture | 2443 B | 2444 B |
| zstd level 3 | 883 B | 844 B |
| 200 encodes, debug CI | 3036 µs | 3285 µs |
| 200 decodes, debug CI | 5735 µs | 7575 µs |

CBOR compressed this fixture 39 bytes smaller, while named MessagePack was faster in both measured serialization directions, especially decode. The size difference is small enough that record locality, compatibility policy, decode behavior and implementation simplicity dominate the choice. Absolute CI timings are not product latency targets.

Canonical MessagePack+zstd measurements on the same Linux CI runner:

- 200 full record encodes: 14430 µs;
- 200 full record decodes: 10295 µs;
- 128 independently framed records: 118795 bytes;
- 1000 catalog-style direct-offset reads: 50973 µs;
- probe process VmHWM: 8004 KiB;
- maximum declared uncompressed record: 32 MiB;
- legacy 1.0 migration: validated rewrite to 1.1;
- future minor and future major versions: rejected.

Rust, Platform shells, and Android ARM64 ABI cross-build all passed at the exact host-qualification SHA.

Android runtime qualification then passed on exact commit `bf3f4aaa52f19be93f527f099dfd57766c3db520`, [Android record codec probe run 38020692235](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38020692235), using an API 35 x86_64 software emulator. The same fixture produced:

| Android API 35 measurement | MessagePack | CBOR |
| --- | ---: | ---: |
| Uncompressed fixture | 2443 B | 2444 B |
| zstd level 3 | 883 B | 844 B |
| 200 encodes | 15706 µs | 19025 µs |
| 200 decodes | 52234 µs | 72322 µs |

Canonical codec Android measurements:

- 200 full record encodes: 6977246 µs;
- 200 full record decodes: 1554813 µs;
- 128 independently framed records: 118795 bytes;
- 1000 direct-offset random reads: 3938487 µs;
- probe process VmHWM: 5628 KiB;
- maximum declared uncompressed record: 32 MiB;
- legacy 1.0 migration: validated rewrite to 1.1;
- future minor and future major versions: rejected.

The emulator uses software CPU virtualization, so these absolute latency figures are **not** real-device performance targets. They are sufficient to qualify Android format compatibility, bounded memory behavior, direct-offset access and migration semantics. Real-device performance remains a later product-performance concern.

## Alternatives considered

**CBOR + zstd.** Very similar raw size and slightly smaller compressed fixture, but slower in the measured encode/decode probe. Retained as a benchmark comparator, not the canonical wire format.

**Whole-shard compression.** Rejected because one article lookup would require scanning or inflating unrelated records, conflicting with bounded mobile random access.

**Uncompressed records.** Rejected because the measured fixture shows substantial record-level compression benefit and Wikipedia text/HTML is highly compressible.

**Implicit schema evolution with best-effort forward decode.** Rejected. Unknown storage semantics must fail closed; supported legacy formats require an explicit migration path.

## Consequences

- `wiki-model` stays independent of storage libraries.
- `wiki-store` owns physical encoding, size limits, framing and migration classification.
- STORE-001 can index frame offsets in SQLite without changing the record wire format.
- Pack/app updates must respect codec major/minor compatibility rather than assuming Serde shape compatibility.
- Android runtime qualification is complete for codec compatibility/memory/random-read behavior; later real-device work may refine performance expectations without changing the wire-format decision.
