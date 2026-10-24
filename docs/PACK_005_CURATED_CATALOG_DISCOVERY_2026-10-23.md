# PACK-005: Curated seed catalog discovery (partial)

**Date:** 2026-10-23  
**Implementation SHA:** `034a32e6266b6b84f06d42d6e200bce21d3f88d3`  
**Scope:** Offline discoverability of curated seed definitions and strict local preview generation labels.

## Behavior

- `wiki-pack list-curated` prints the current editable Mathematics,
  Physics and Computer Science seed IDs, display names, `enwiki` edition,
  category roots and depth/page caps.
- `wiki-pack list-curated --json` returns schema-versioned, deterministic
  JSON for tooling/GUI consumers. It explicitly marks source authentication,
  publisher signature verification, offline readiness and installation **false**.
- Neither command opens a source, downloads data, builds a pack or claims
  Wikimedia authority.
- `wiki-pack preview-topic` with `curated_topics` now rejects empty
  (`enwiki-`), punctuation-only, malformed and oversized generation
  suffixes. The snapshot label remains a caller claim, not an official
  publication proof.

## Tests and qualification

- CLI unit tests assert deterministic JSON, one-to-one correspondence with
  `INITIAL_TOPICS`, exact root/limit fields and explicit false trust claims.
- Human-readable output tests check each seed and the trust disclaimer.
- Curated preview tests reject invalid generation suffixes and preserve valid
  `enwiki-20261023` fixture behavior.
- Initial SHA `31e04909d680fb6bbabcc1da53da1102292560c6` failed
  [Rust CI 40067620199](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40067620199)
  at `cargo fmt --all -- --check`; corrected SHA
  `034a32e6266b6b84f06d42d6e200bce21d3f88d3` passed
  [Rust workspace 40068006127](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40068006127)
  (format, strict Clippy and tests). [Platform shells 40068006138](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40068006138)
  was running at documentation time; recheck its terminal outcome.

## Not qualified

The local category graph remains caller-supplied and untrusted. No authenticated
Wikimedia category completeness, measured real-source coverage/fan-out,
revision-matched mandatory media, production pack construction, UI integration,
or physical-device acceptance has been demonstrated. **PACK-005 remains unchecked.**
