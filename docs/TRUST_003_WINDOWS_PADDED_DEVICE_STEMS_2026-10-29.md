# TRUST-003 / PACK-001 — Windows padded device-name stem hardening (2026-10-29)

## Problem and scope

The provisional unsigned `.wpack` manifest path and pack-identity validator recognized Windows reserved DOS device names such as `CON`, `NUL`, `COM1`, `LPT9`, `CONIN$` and `CONOUT$`, including their ordinary extension forms. It did not normalize trailing ASCII spaces **in the device-name stem before an extension**. Consequently a hostile pack could supply `media/CON .txt` or `media/COM1 .svg` that passed the shared manifest validator even though Windows path handling can interpret the stem as a device name.

The common `unsafe_component` helper now trims ASCII spaces at the end of the stem before testing reserved device names, including Windows superscript-digit port variants. The helper is reused for pack IDs, origin metadata, object-path components and staging-directory names. It does not broadly ban legitimate names containing spaces, and does not alter the binary format.

## Synthetic tests

The manifest regression rejects `CON .txt`, `con  .svg`, `NUL .json`, `COM1 .bin`, `com9  .log`, `LPT² .png`, `CONIN$ .txt` and `CONOUT$  .dat` as object paths and pack IDs, while retaining `CONtent .svg` and `COM10 .txt`. Staging tests reject padded device names before inspecting an absent parent directory or consuming input. These are cross-platform unit tests; no claim of actual Windows device qualification is made.

## Exact-head verification

- First source SHA `24d8501d5fcada255362d05f007ee49ef052281c` failed only Rust formatting: [Rust workspace 40896735757](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40896735757).
- Corrected source SHA **`04d99cc1c4fd9c7514291872b892b9e1f6fc1764`** passed [Rust workspace 40897069175](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40897069175), [Platform shells 40897069108](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40897069108) and [Android Rust ABI 40897069190](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40897069190).

## Remaining acceptance

This is a narrowly scoped hostile-input hardening slice, not a complete `.wpack` implementation, verified publisher signing, mandatory offline visual media, real pack activation, Windows/macOS filesystem qualification, or physical Android acceptance. Keep **TRUST-003** and **PACK-001** unchecked.
