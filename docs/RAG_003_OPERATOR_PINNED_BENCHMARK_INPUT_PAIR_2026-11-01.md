# RAG-003 — Operator-pinned retrieval benchmark inputs

**Date:** 2026-11-01 (America/Los_Angeles)  
**Source commit:** `ece809e88a85e6f980aa08c06d5aafa4e4e42808`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem

The offline retrieval runner previously included content-addressed SHA-256 receipts in its schema-v3 output, but accepted whatever corpus and label suite the caller supplied. A mistaken path or changed label file could be scored under the same caller-provided Git SHA, with the discrepancy only discoverable after the run. The Git SHA identifies code, not the evaluated data.

## Implementation

Two optional arguments are now supported together:

```text
--expect-articles-sha256 <64-hex>
--expect-suite-sha256 <64-hex>
```

The runner validates 64 hexadecimal digits for each, normalizes uppercase to lowercase and rejects a single unpaired pin. Once the canonical article objects have been loaded and the bounded label-suite bytes read, it checks both expected digests **before** retrieval evaluation and before publishing any report. A mismatch fails without writing a misleading report. Existing unpinned runs remain backward-compatible.

The articles digest is SHA-256 of the deterministic JSON serialization of canonical `Article` objects, sorted by project, page ID and revision ID. It is **not** necessarily SHA-256 of the original corpus file, the compressed pack, or the SQLite snapshot. The label digest is SHA-256 of the **exact raw suite bytes**, including whitespace. The existing schema-v3 `input_receipts` records both actual digests.

### Reproducible workflow

First run the benchmark against the intended corpus/snapshot/pack and label suite with a fresh report path, and review the `input_receipts.canonical_articles_json_sha256` and `input_receipts.label_suite_bytes_sha256` fields. For a subsequent comparison, supply those two values with the new flags and a **new** report output path. A changed article revision, corpus object or label byte sequence then prevents scoring. Record the expected digests in the evaluation protocol and retain the original source and label-review evidence separately.

These pins are **operator assertions**. They cannot authenticate a Wikimedia publisher, validate the correctness of human labels, prove generated-answer entailment, or establish device performance.

## Regression evidence

Added CLI tests for unpaired/malformed digests, normalization and deterministic comparison. The existing real SQLite snapshot-backed benchmark test now runs with correct pins, verifies successful publication, then supplies an incorrect label digest and asserts rejection with **no output file**.

The first implementation commit `b6182c036a08ebd42276194873aed9e117a4c1c9` failed rustfmt ([Rust workspace 41402755763](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41402755763)). The formatting correction `b25b96603649068f5230f5cab60958776186fb30` exposed a moved test fixture ([Rust workspace 41403080938](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41403080938)); the final source fixed the fixture.

**Qualified exact source commit:** [`ece809e88a85e6f980aa08c06d5aafa4e4e42808`](https://github.com/ekkus93/foundation-wikipedia/commit/ece809e88a85e6f980aa08c06d5aafa4e4e42808).

All three exact-head workflows passed:
- [Rust workspace — 41403417550](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41403417550) (format, Clippy, workspace tests).
- [Platform shells — 41403417569](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41403417569).
- [Android Rust ABI — 41403417552](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41403417552).

## Remaining RAG-003 work

Build a representative independently reviewed Wikimedia-labeled evaluation suite with authenticated source-generation provenance, measure answer-level factual support and citation precision rather than just retrieved passage relevance, and qualify real desktop/Android memory, latency and offline behavior. The desktop AI-005 chat integration remains a separate high-priority milestone.
