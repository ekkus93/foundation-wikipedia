# STORE-001 Android codec emulator qualification — 2026-10-14

The Android record codec probe passed on the exact implementation head `5af57b9e8304d0515868d295a3ccabb4cb7ce552`.

CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38573724607

The job cross-built the Rust codec probe for Android x86_64 using the pinned NDK, ran it on an API 35 emulator, preserved evidence, and confirmed exact head. This closes the previously pending emulator probe only; full STORE-001 random-access snapshot and real-device acceptance remain incomplete. Do not mark STORE-001 done.
