---
name: lint-n-test
description: Lint the files and run all tests (rustfmt check, clippy, cargo tests, Python script tests, fixture provenance) and report results.
model: haiku
---

Run from the repo root, in order. Continue past a failure so every result is reported:

1. Lint: `cargo fmt --all -- --check`
2. Lint: `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. Test: `cargo test --workspace --locked`
4. Test: `python3 -m unittest discover -s scripts`
5. Test: `python3 scripts/verify_fixture_provenance.py`

Finish with a table of pass/fail per step, and include the relevant error output for any failure. Do not edit code or fix failures unless asked. Never report a step as passing if it did not run.
