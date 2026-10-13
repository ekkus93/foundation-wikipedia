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
