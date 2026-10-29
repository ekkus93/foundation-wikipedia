# PACK-004 / TRUST-003 — Bounded read-only stdin verification (2026-10-29)

## Implementation

The provisional `wiki-pack` CLI now supports piped, nonseekable envelope verification **without writing an archive spool or staging directory**:

```sh
cat custom.wpack | wiki-pack verify-stdin 104857600
cat custom.wpack | wiki-pack inspect-stdin 104857600
```

Both commands require one decimal `max-input-bytes` argument. The budget includes the 8-byte envelope magic, 4-byte transcript length, canonical transcript and every declared object byte. The shared `wiki-pack-format::container::verify_container_with_budget` wraps the existing full envelope/digest verifier in a read limiter. At an exact budget boundary it can consume **one additional probe byte** from stdin to distinguish a legitimate EOF from excess input, but it does not retain that byte. A zero budget is rejected before reading. The verifier does not need `Seek`, does not create temporary files, and does not install or activate any pack.

`verify-stdin` prints the object count, declared bytes and explicit publisher/media verification limitations; `inspect-stdin` additionally prints pack identity and the **unauthenticated** claimed origin. The existing `verify <file>` and `inspect <file>` commands retain their behavior.

## Black-box regression

`tools/wiki-pack/tests/verify_stdin_cli.rs` invokes the real executable with piped stdin. It checks exact-byte-budget success, detailed identity inspection, an unverified official-origin claim, one-byte-under-budget and zero-budget rejection, malformed decimal arguments, object digest mismatch and appended trailing bytes. The new API is a local-integrity check only; no publisher signatures, media completeness or installed-pack state are inferred.

## Exact source and CI

- First code SHA `d577d37323c46c0c64788d53c9d3df755963e7d7` failed formatting in [Rust workspace 40880959527](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40880959527).
- Second SHA `7e65591e7dee204cf9fa0df8eda07cef47bc56d6` failed formatting on an extra blank line in [Rust workspace 40881049630](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40881049630).
- Corrected exact code SHA **`694ee2fd02111b722582991ad574efdbd03ffa76`** passed [Rust workspace 40881156229](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40881156229) (rustfmt, strict Clippy, workspace tests) and [Platform shells 40881156262](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40881156262).

## Remaining acceptance

The byte-budgeted stdin verifier is not an Android SAF integration, signed official pack verifier, media-completeness proof or installation/activation pipeline. `PACK-004` and `TRUST-003` remain unchecked pending those independent requirements and real-device acceptance.

## Follow-up — manifest allocation preflight

The bounded verifier now rejects an attacker-controlled transcript length that cannot fit within the declared total input-byte budget **before allocating** the transcript. It reads only the 12-byte envelope header for this preflight and then replays those bytes into the unchanged full verifier. A new unit regression supplies a 64-MiB declared transcript with a 32-byte budget and asserts `ResourceBudget` after consuming exactly 12 bytes.

Initial source `7e79fde528527a501aa00c9a18e2ece04daa1697` failed formatting ([Rust 40882454558](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40882454558)); corrected exact source **`a0af3e078c36db8d501c27b92e9a2bcac63a325d`** passed [Rust workspace 40882587232](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40882587232), [Platform shells 40882587233](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40882587233), and [Android Rust ABI 40882587584](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40882587584). This remains a local integrity/memory-safety check, not signed-pack or installed-offline qualification.
