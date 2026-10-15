# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Layout

Cargo workspace (`crates/*`, `tools/wiki-pack`), toolchain pinned to 1.85.1 by `rust-toolchain.toml`, `unsafe_code = "forbid"`. `desktop/` (Tauri 2 + React/Vite, Rust 1.90) and `android/` (Kotlin/Compose) are bootstrap previews outside the workspace. The nine library crates must not import Tauri or Android APIs. Most active work is in `crates/wiki-source`.

## Verify before committing

Run in CI order; all must pass:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s scripts && python3 scripts/verify_fixture_provenance.py
```

- Run `cargo fmt --all` before committing. Formatting failures have broken CI before (`docs/CI_FORMAT_FIX_2026-10-09.md`).
- Unit tests need no network, LLM, or downloaded dataset.
- Every file under `fixtures/` needs a `<filename>.provenance.json` sidecar (source type, license, SHA-256 of exact bytes; Wikimedia fixtures also record URL, project, page ID, revision ID). A missing or mismatched sidecar must fail validation.
- Each `scripts/*.py` has a `test_*.py` beside it.

## Commits and CI

- Conventional commits, lowercase imperative: `type(scope): message`, e.g. `test(src-003): fix CR fixture and document recovery`. Scope is a crate area (`source`) or a TODO item ID (`src-003`).
- Work integrates directly on `master`. After pushing, check CI separately on the exact pushed head.
- A TODO item counts as complete only with committed qualification evidence from exact-head CI. The usual cycle is docs record the gap, then a failing regression test, then the fix, then docs qualify it. Evidence docs are named `SRC_003_<TOPIC>_YYYY-MM-DD.md`.
- Spec is `docs/WIKIPEDIA_AI_READER_SPEC.md`; the canonical checklist is `docs/WIKIPEDIA_AI_READER_TODO.md`. Don't edit the TODO casually.
- An uncertain GitHub write acknowledgement or a read timeout is not evidence the mutation failed. Re-read the head before acting; never blindly retry ambiguous writes (`docs/RALPH_BRIDGE_WRITE_AND_CI_INCIDENTS_2026-10-15.md`).

## Gotchas

- Public-dump revisions with timestamps after the pinned dump date or publication day are rejected, in both the XML guard and diagnostic JSONL. The generation date must be a valid calendar date.
- Diagnostic revision JSONL is LF-only; a CR in a record is rejected. `audit_public_dump_revision_jsonl.py` enforces size caps and does NOT authenticate Wikimedia publication.
- Dump siteinfo project/database identity must agree (fail-closed). The deletion decoder rejects duplicate page IDs.
- The curl transport never follows redirects, never disables cert verification, and accepts only canonical read-only Action API URLs. It needs curl 7.63+ and is used only by live smoke workflows; Android has no curl.
- Cached HTML is temporary and keyed by revision; it is not an offline-ready article.
