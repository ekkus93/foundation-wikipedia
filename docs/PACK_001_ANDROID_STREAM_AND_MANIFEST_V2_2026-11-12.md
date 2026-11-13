# PACK-001 — Android streamed install, accepted framing and manifest v2

**Date:** 2026-11-12  
**Repository:** `ekkus93/foundation-wikipedia`  
**Code SHAs:** `5b6e5529` (Android streamed export/install probe), `713d84e3` (ADR 0007 accepted), `01a7f343` (manifest v2 metadata)

## Owner decisions (2026-11-12)

- [ADR 0007](adr/0007-wpack-container-framing.md) is **accepted** as the v0.1 `.wpack` framing.
- The manifest gains source, definition, licence, index and count metadata as **transcript version 2**. Version 1 is rejected outright, because no v1 packs shipped.

## Android streamed export and install (`5b6e5529`)

`crates/wiki-store/examples/pack_stream_probe.rs` is cross-built for `x86_64-linux-android` (NDK 27.2.12479018, API 26 linker). `.github/workflows/android-codec-probe.yml` runs it on an API 35 `google_apis` emulator under `/data/local/tmp`.

The probe:
1. exports 200 articles to a `.wpack` file by streaming;
2. installs that file through a `Read`-only adapter with no `Seek`, as an Android content-provider stream arrives;
3. checks the installer rebuilt the omitted FTS index;
4. activates the snapshot, opens page 42 through the verified reader and searches;
5. confirms a one-byte-short spool budget and a one-bit-tampered payload are both refused, with nothing published and an empty `staging/`.

Output on the Android emulator, from [Android record codec probe 43380843701](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43380843701):

```
pack_stream_probe export articles=200 wpack_bytes=158915 export_ms=127
pack_stream_probe refusals budget=refused tampered=refused published=none staging=empty
pack_stream_probe install stream=nonseekable trust=CustomIntegrityVerified rebuilt_index=true install_ms=9844 active=orbits page42_revision=5000042 search_hits=50
pack_stream_probe result=pass
```

The emulator install took 9.8 s, against 67–75 ms on the Linux host. The CI emulator probably runs under software CPU emulation (`swiftshader_indirect` is set for graphics), so this timing is not a device performance figure.

All four workflows passed on `5b6e5529`:

| Workflow | Run |
| --- | --- |
| Rust workspace | [43380843492](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43380843492) passed |
| Platform shells | [43380843463](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43380843463) passed |
| Android Rust ABI qualification | [43380843655](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43380843655) passed |
| Android record codec probe | [43380843701](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43380843701) passed |

## Manifest v2 (`01a7f343`)

- **`wiki-pack-format::manifest`** adds `PackMetadata`, carrying:
  - `SourceSnapshot` (kind, `snapshot_id`, `manifest_sha256`);
  - `definition_sha256`;
  - `License` (SPDX text licence and an optional attribution object);
  - `IndexEntry` list (path, role catalog/FTS5, schema);
  - `Counts` (articles, redirects, media).

  Validation requires lowercase SHA-256s, a safe snapshot ID, a bounded SPDX token and declared, unique index paths and roles with schema at least 1. It also requires an attribution object whenever media are declared, and the attribution object must not also be an index. Every field is bound into the v2 transcript, and the index order is canonical.
- **`container`** parses v2. It rejects unknown source, attribution or role tags and over-limit index counts, as well as the v1 prefix and version.
- **`wiki-store::pack_install`**:
  - locates indexes by declared role, accepting only schema 1 and refusing undeclared index files;
  - excludes the attribution object from shards;
  - refuses packs declaring media (`MediaNotSupported`);
  - requires declared article and redirect counts to equal a verified catalog scan (`CountMismatch`);
  - records the metadata in the installer's `manifest.json`.

  `snapshot_pack_metadata` derives indexes and counts from a snapshot tree for builders.
- **`wiki-pack export-custom`** takes a strict, size-bounded `metadata.json` (unknown fields rejected). `inspect` prints the metadata in its detailed human and JSON output.

Regression evidence:

- `crates/wiki-pack-format/tests/manifest_v2_metadata.rs` covers:
  - a full round trip;
  - canonical index order;
  - each of ten metadata edits changing the identity transcript;
  - thirteen invalid-metadata cases;
  - unknown source tags rejected in the envelope.
- `container::tests::rejects_forged_header_or_noncanonical_manifest` now also rejects a v1 version field and a v1 transcript prefix.
- `pack_install::tests::manifest_metadata_is_checked_against_the_snapshot` covers wrong article and redirect counts, an undeclared search index, a future catalog schema, a missing catalog role and a media-bearing pack. It also checks that a text-only pack with an attribution object installs and keeps that object out of the shards, and that the installer manifest records v2 metadata.
- The CLI export tests pass a metadata file. Every existing pack, installer, harness and probe test now builds v2 manifests.

Local verification (Debian 13, Rust 1.85.1, `umask 022`) passed all five checks: rustfmt, clippy `-D warnings`, workspace tests, Python tests (636) and fixture provenance. The host probe printed `result=pass`.

**CI on `01a7f343`:** [Rust workspace 43382314227](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43382314227), [Platform shells 43382314197](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43382314197), [Android Rust ABI 43382314251](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43382314251) and [Android record codec probe 43382314314](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43382314314) all passed.

## Boundaries

- **PACK-001 stays unchecked.** Its acceptance needs a round trip through an **independent** importer. The probe and installer share one Rust implementation.
- Source and definition hashes are the builder's claims, not checked against an authenticated upstream (SRC-002/003). The spec does not yet define the canonical topic-definition form that `definition_sha256` covers (PACK-002/005).
- Pack media installation and the attribution content schema belong to PACK-003. Signatures over the v2 transcript belong to TRUST-001.
- The Android result comes from a CI emulator, not a physical device or a real document-picker stream (MOBPACK-002).
