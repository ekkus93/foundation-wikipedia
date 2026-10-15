---
name: verify
description: Run the full pre-commit verification sequence (rustfmt, clippy, workspace tests, Python script tests, fixture provenance) in CI order and report failures. Use before declaring a change done or committing.
---

Run these from the repo root, in order, stopping to fix and re-run on the first failure:

1. `cargo fmt --all -- --check` (if it fails, run `cargo fmt --all` and re-check)
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `cargo test --workspace --locked`
4. `python3 -m unittest discover -s scripts`
5. `python3 scripts/verify_fixture_provenance.py`

Report each step as pass/fail with the relevant error output for any failure. Never report success for a step that did not run or that failed. Unit tests need no network; do not run live-smoke workflows.
