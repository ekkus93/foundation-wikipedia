# Desktop online reader integration: Rust URL dependency compatibility

An initial Tauri → shared `wiki-source` command adapter was tested directly on `master` on 2026-10-13. It implemented bounded search and revision-pinned article commands and an isolated frontend text preview, but **did not qualify**: desktop Tauri CI and desktop launch smoke failed dependency resolution. The experiment was reverted to restore a green master; no UI functionality is claimed.

## Reproducible failure

- Experiment commits: `be28ba1a576c38ae337e356c9177e80982163fca` (Tauri source adapter), `5fac93e005010d8823696df8c5438511d8832e80` (frontend preview).
- Failing Platform shells CI: `38476864344` and `38477045066`. Desktop launch smoke: `38476864168` and `38477045039`.
- Tauri's `tauri-runtime-wry → wry` requires `url ^2.5`; the shared `wiki-source` crate pins `url =2.4.1` to preserve the root Rust 1.85.1 minimum supported toolchain. Cargo cannot resolve these constraints together in one Tauri crate. The workspace Rust CI still passed, but that does not qualify the Tauri dependency graph.

## Required next work

1. Evaluate an exact `url 2.5.x` release and its transitive `idna`/ICU dependencies against the root pinned Rust 1.85.1 and Android ABI builds. Update the root lockfile deterministically and run full exact-head CI. If the MSRV cannot be preserved, record and qualify a deliberate toolchain upgrade across host and Android rather than silently breaking the shared core.
2. Restore the thin Tauri adapter only after the shared source and Tauri dependency graphs can resolve together. Keep blocking HTTPS transport off the UI thread, return typed navigation metadata, and retain strict source revision matching.
3. Render sanitized article HTML only inside a sandboxed, network-restricted viewer until required visuals are locally verified. Keep links and privileged Tauri APIs isolated, and never claim offline completeness for HTML-only cache.
4. Qualify actual desktop launch, live Wikimedia interoperability, media behavior, and eventually physical-device/offline acceptance. SRC-004/DUI-001 remain unchecked.

The opt-in source-level live smoke harness added separately remains on master and is independent of this Tauri dependency conflict.

## Resolution (2026-10-13)

The `url` dependency conflict has been resolved without changing the root Rust 1.85 toolchain. `wiki-source` now accepts `url 2.x` while its root lockfile retains 2.4.1; the separate Tauri/Wry graph can resolve `url ^2.5`. The dependency fix `ae8f03e77c4b4b09a4d692f15a98b0245ed492e3` passed Rust CI 38479192102, Platform CI 38479192041, and dependency probe 38479192222.

The Tauri source adapter and sandboxed online text preview were restored at `c227c9a2ca45ce70ad0d588dd634ef07eb10fa4e`, passing Rust CI 38479292841, Platform CI 38479292844, and desktop launch smoke 38479292815. Explicit offline request suppression at `fda67d73180cf38a4d17e674d4d1e76a5ac61c78` passed Rust CI 38479398977, Platform CI 38479398936, and desktop launch smoke 38479398922. Host curl stderr hardening was qualified through `6b0a150f7db2a1887e94d5e310a19d40e0f3e960` (Rust CI 38479837982, Platform CI 38479837719).

This qualifies dependency resolution, deterministic tests and desktop startup, **not** a successful live Wikimedia request or offline-complete article reader. SRC-004 and DUI-001 remain unchecked. The original failed experiment and recovery are retained above as historical evidence.
