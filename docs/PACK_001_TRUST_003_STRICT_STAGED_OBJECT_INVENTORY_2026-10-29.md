# PACK-001 / TRUST-003 — Strict staged-object inventory (2026-10-29)

## Scope

The public `wiki_pack_format::objects::verify_manifest_directory` now rejects undeclared files and directories in a supposedly verified pack staging tree. Previously it verified hashes and sizes of all declared objects but tolerated unrelated entries; an installer reusing the tree could inadvertently process bytes outside the manifest's integrity scope. This is an integrity-hardening slice, **not** publisher authentication, archive installation, or proof of mandatory offline media coverage.

The verifier derives a set of canonical declared file paths and their necessary parent directories from a validated manifest, walks the staged directory without following symlinks, and rejects any additional regular file or directory. Symlinks and special filesystem objects remain `UnsafePath`; unexpected ordinary entries produce `UnexpectedObject(relative_path)`. Object hash, length, hardlink, and ancestor-symlink checks remain in force. This still assumes the caller exclusively owns an immutable staging tree; descriptor-relative race protection and physical power-loss recovery are not established.

## Synthetic regression

The Rust `wiki-pack-format` unit test `objects::tests::rejects_unlisted_staging_files_and_empty_directories` verifies an exact single-object tree, adds an undeclared file and confirms rejection, removes it, adds an empty undeclared directory and confirms rejection, then confirms the clean tree still verifies. Existing hardlink and symlink rejection tests continue to run.

## Exact source and CI

- Initial code SHA `12c6ccdb5ed89e970e69a9b815f3ceb745367867` failed rustfmt ([Rust 40875642719](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40875642719)).
- Formatted SHA `6665c969457d8c6c8c4bbdb5d5f50c4d69af50ac` exposed an existing symlink error-classification regression ([Rust 40875726795](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40875726795)).
- Corrected exact source SHA **`b1a4516a810e74c332d8a0806f1f257d032669fe`** passed [Rust workspace 40876007220](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40876007220) (format, strict Clippy and tests), [Platform shells 40876007230](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40876007230), and [Android Rust ABI qualification 40876007219](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40876007219).

## Remaining qualification

Publisher-signed origin, mandatory visual/media discovery and attribution, production install/activation, complete hostile-import review, race-resistant directory ownership, and real desktop/Android offline acceptance remain open. **PACK-001 and TRUST-003 remain unchecked.**
