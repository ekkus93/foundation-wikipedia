# PACK-004 / TRUST-003 — Bounded nonseekable CLI staging (2026-10-29)

## Implementation

The provisional `wiki-pack` CLI now exposes:

```sh
cat custom.wpack | wiki-pack stage-unsigned-stdin /path/to/private-parent staged-name 104857600
```

The final argument is a mandatory decimal spool-byte quota, including envelope header, manifest and objects. This calls the shared `stage_container_stream_with_budget` verifier with stdin as a nonseekable `Read` stream, so it does not load the archive into RAM or require `Seek`. Invalid bytes, exceeded budgets and read failures do not produce a staged pack; the spool is removed. Output explicitly says **UNTRUSTED STAGING ONLY**, **NOT INSTALLED**, publisher signature **NOT VERIFIED**, and required offline media **NOT VERIFIED**, even for an official-origin claim.

This command is a CLI integration for document-provider-like streams; it does not yet connect Android's Storage Access Framework or install a pack. Its private parent must be caller-owned and exclusive, consistent with the Rust staging API.

## Black-box regression

`tools/wiki-pack/tests/stage_stdin_cli.rs` invokes the actual CLI executable with piped stdin. A canonical unsigned custom pack stages at the exact byte quota, preserves object bytes and prints nonpublisher trust status. A one-byte-smaller quota and a corrupted object both fail without publishing a new staging directory or leaving a temporary spool.

## Exact source and CI

- Initial code SHA `29b7bb55f66237e5bafc12c43a6c3715a2559fa4` failed rustfmt ([Rust workspace 40877432898](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40877432898)).
- Corrected exact code SHA **`b173cf4058e9e82e808ab55a30fef6e975519612`** passed [Rust workspace 40877790592](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40877790592) (format, strict Clippy and tests). [Platform shells 40877790606](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40877790606) was in progress at this evidence capture; do not count it as passed until terminal.

## Remaining

This is not authenticated official publication, offline visual completeness, atomic pack activation, Android SAF integration, or physical-device acceptance. `PACK-004` and `TRUST-003` remain unchecked.
