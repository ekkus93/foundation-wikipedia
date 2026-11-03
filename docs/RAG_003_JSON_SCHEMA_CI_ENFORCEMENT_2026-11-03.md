# RAG-003 — Enforce review-manifest JSON Schema in CI

**Date:** 2026-11-03 (America/Los_Angeles)  
**Qualified source commit:** `fc01136a57ded51d1eca5a605a6ab21b071e9a03`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem

The repo already included `docs/schemas/rag003-review-manifest.schema.json` (Draft 2020-12) and a separate Python semantic validator. Until this increment, running `scripts/validate_rag003_review_manifest.py` did **not** validate the manifest against the structural schema, so missing required fields, unknown properties, wrong data types, and malformed source URLs could escape the command's declared validation contract.

## Implementation

The CLI now calls `validate_manifest`, which runs `validate_structure` first and semantic checks only if structural validation succeeds. Structural enforcement uses `jsonschema.Draft202012Validator` with `FormatChecker` and the **checked-in** schema path relative to the validator script; no network schema lookup is required. It limits displayed structural errors to the first 20, with bounded error text. The CLI also bounds the manifest read to 2 MiB, parses UTF-8 strictly, and continues rejecting duplicate JSON object keys.

CI installs the pinned Python dependency in a dedicated `.rag003-venv` and runs both the full script test discovery and the explicit manifest gate through that interpreter. `scripts/requirements-rag003.txt` specifies `jsonschema[format]==4.23.0`.

**Important qualification discovery:** The first implementation installed `jsonschema==4.23.0` without its optional format validators. An invalid date-time regression failed because JSON Schema `format` checks were silently unavailable. The corrected dependency includes the `[format]` extra so timestamp and URI format assertions are actually enforced. This failure is recorded, not suppressed.

## Regression coverage

The additional tests verify:

- A structurally complete synthetic manifest passes both structural and semantic validation.
- Missing required source URLs, undeclared fields, and incorrectly typed citation flags fail structural validation.
- Invalid RFC 3339 timestamps and non-HTTPS source URLs are rejected (including enforced format assertions).
- A structurally valid manifest with a nonexistent source revision still fails semantic validation.
- The command-line entry point rejects unsupported schema versions, duplicate JSON keys, and oversized input.

The fixtures deliberately contain synthetic source and reviewer data; they are **not** independent Wikimedia evaluation evidence.

## Verification

Source `fc01136a57ded51d1eca5a605a6ab21b071e9a03`:
- [Rust workspace CI — 41701984482](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41701984482) — **passed**, including pinned schema dependency setup, broad unit tests and the explicit RAG-003 gate.
- [Platform shells CI — 41701984512](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41701984512) — in progress at initial documentation.

The initial source SHA `78bc090849ca599620d3f2b49fcb6de49e1bccbf` failed the date-time format regression in [Rust workspace run 41701543892](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41701543892). The corrected source above includes the format-validation dependency.

For direct usage after installing the pinned requirement into a virtualenv:

```sh
python3 -m venv .rag003-venv
.rag003-venv/bin/python -m pip install -r scripts/requirements-rag003.txt
.rag003-venv/bin/python scripts/validate_rag003_review_manifest.py review-manifest.json
```

## Remaining acceptance work

The validator checks declared schema structure and a subset of cross-reference/chronology consistency only. Neither the manifest nor the validator authenticates Wikimedia revision bytes, establishes genuine independent reviewers, verifies the actual content SHA-256 values, proves correct claim spans against the model answer, or grades factual entailment. Obtaining and auditing representative real evaluation data, reconciling the exact corpus/annotation fixtures, and measuring desktop/Android performance remain required. Do not mark RAG-003 complete from synthetic CI alone.
