# ADR 0007 — `.wpack` container framing and streamed install

**Status:** Accepted (owner decision, 2026-11-12) as the v0.1 `.wpack` framing, as implemented in `wiki-pack-format::container` (`FWPACK01`, manifest identity transcript v1).  
**Date:** 2026-11-12  
**Requirements:** PACK-001, TRUST-001, TRUST-003, MOBPACK-002, STORE-003

## Context

PACK-001 asks for a portable, versioned `.wpack` container, an evaluation of safe streamed export and install on Android, and an ADR recording the chosen archive and serialization. Android imports custom packs through the system document picker (MOBPACK-002, spec §18), which yields an **unseekable** content-provider stream. Packs are untrusted input until TRUST-001 exists (spec §8, §21).

## Decision

**Envelope:**

1. 8-byte magic `FWPACK01`.
2. Big-endian `u32` length of the manifest identity transcript. The length must be 1 to 64 MiB (`MAX_MANIFEST_BYTES`).
3. The canonical **identity transcript**:
   - prefix `foundation-wikipedia/manifest-identity/v1\0`;
   - `u32` format version (1);
   - length-prefixed (`u64` BE) UTF-8 fields: `pack_id`, `project`, `snapshot`;
   - an origin tag: `1` = Official (`publisher`) or `2` = Custom (`definition_id`);
   - a `u64` object count (1 to 1,000,000);
   - for each object, sorted by byte-wise path: `path`, lowercase hex `sha256`, `u64` byte length.
4. The raw object bytes, concatenated in that same canonical path order, with no per-object headers, padding or trailing data.

**Canonical form.** A parser re-serializes the manifest and rejects any transcript that is not byte-identical. That rules out alternate order, digest casing and encodings, so one pack identity has exactly one encoding. Manifest validation enforces:
- safe IDs and object paths (at most 4,096 bytes, 64 components of up to 255 bytes, NFC, portable Windows names, no Unicode aliasing, no invisible or spoofing characters);
- unique paths and consistent digests;
- object and total size ceilings (1 TiB per object, 4 TiB declared).

**Serialization.** There is no envelope-level compression. Article records are already individually zstd-compressed (`FWREC001`, ADR 0001), media is stored in its own formats, and the derived SQLite files are copied as-is. That keeps object bytes directly hashable and installable without a decompression pass.

**Verification.** It streams. Each object is hashed while it is read, with bounded buffers, and the envelope must end exactly after the last object. `verify_container_with_budget` enforces a caller byte quota: it rejects an oversized declared transcript or object set before allocating or consuming payload.

**Staging is two-pass and fail-closed** (`stage::stage_container*`):
1. Verify the whole input.
2. Recheck the identical transcript.
3. Write objects into a freshly created private (0700/0600) directory.
4. Re-verify the staged tree.
5. Sync files and directories.

Any failure removes the partial tree durably.

**Unseekable streams.** `stage_container_stream_with_budget` tees the stream into a private `create_new` spool file while verifying it under the quota. It then stages from the spool through the same two-pass path, requires that the staged and streamed manifests are identical, and deletes the spool. `pack_install::install_custom_pack_stream` builds the STORE-003 installer on top of this.

**Export.** `export::write_custom_tree` streams a private, immutable tree into the envelope. It rechecks object membership and digests against the manifest so a changed tree is not exported.

**Trust.** The envelope is unsigned. Origin is a claim. The installer refuses official-origin packs until TRUST-001 and labels custom packs "Custom — integrity verified" only after full byte verification.

## Alternatives considered

- **ZIP:** its central directory sits at the end of the file, so an unseekable stream cannot be validated before payload is consumed without spooling anyway. ZIP also allows duplicate entries, local and central headers that disagree, path traversal, symlink entries, and per-entry compression (decompression bombs). Rejecting all of that would need a hardened subset parser, with none of ZIP's tooling benefits surviving.
- **tar:** header variants (ustar, pax, GNU), link entries, duplicate names and unbounded extension headers present a similar traversal and ambiguity surface. There is also no canonical single encoding of a manifest.
- **A single SQLite archive file:** seekable random access is attractive. But importing from a content-provider stream still needs a full spool, validating an untrusted SQLite file is harder than hashing framed bytes, and objects would have to be extracted again into the snapshot layout.
- **Envelope-level compression (for example a zstd stream):** it would put decompression in the trust path and hide object sizes from the pre-allocation checks. Most payload is already compressed.

## Evidence

- **Format behaviour** is qualified by the earlier PACK-001/TRUST-003 documents (2026-10-29), covering: unseekable stream staging, stage occupancy durability, the manifest invisible-identity guard, portable staging names, strict staged object inventory, the Unicode path-alias preflight and the Windows portable-path preflight.
- **Android streamed export and install:** `crates/wiki-store/examples/pack_stream_probe.rs`. The Android record codec probe workflow cross-builds it for `x86_64-linux-android` (NDK 27.2, API 26 linker) and runs it on an API 35 emulator. On the device filesystem it:
  1. exports 200 articles to a `.wpack` file by streaming;
  2. installs that file through a `Read`-only adapter (no `Seek`, like a content-provider stream) with an exact spool budget;
  3. checks the installer rebuilt the omitted search index;
  4. activates the snapshot, opens page 42 through the verified reader and searches;
  5. shows a one-byte-short budget and a one-bit-tampered payload are both refused, with nothing published and an empty `staging/`.

  The workflow requires these markers:
  - `pack_stream_probe target_os=android target_arch=x86_64`
  - `pack_stream_probe refusals budget=refused tampered=refused published=none staging=empty`
  - `pack_stream_probe install stream=nonseekable trust=CustomIntegrityVerified rebuilt_index=true`
  - `pack_stream_probe result=pass`

  Host run (Linux x86_64, release): a 158,915-byte pack, install in 67 ms, `result=pass`.

## Consequences

- One canonical encoding per pack identity, which gives TRUST-001 a stable byte string to sign.
- Validating before allocating bounds memory, and a byte quota bounds disk on import.
- An unseekable import temporarily needs disk for the spool plus the staged tree, about twice the pack size, until the spool is deleted.
- With no envelope compression, packs are only as compact as their objects (compressed records, media formats, SQLite). A derived `search.sqlite` may be omitted, because the installer rebuilds it (ADR 0006).
- The magic carries format version 1. A future layout change needs a new magic or transcript version, and readers reject unknown versions.

## Not yet established

- The PACK-001 acceptance criterion: a round trip through an **independent** desktop or Android-compatible importer. The emulator probe uses the same Rust implementation, not an independent one.
- A real Android document-picker stream (MOBPACK-002), physical devices, and pack sizes beyond the probe.
- Signatures (TRUST-001), licensing fields and source or definition hashes in the manifest (the first PACK-001 subtask).
