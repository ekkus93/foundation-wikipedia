# RAG-003 — Single-handle bounded benchmark input reads

**Date:** 2026-11-01 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `3aefff254fe5425fa87de0c30ff2341ffd0eeaf6`  
**Status:** Partial. RAG-003 remains unchecked.

## Problem

The offline retrieval suite runner previously checked input size using `fs::metadata(path)` and then reopened the same pathname with `fs::read(path)`. A file replaced or extended between those operations could cause the runner to allocate and read arbitrarily more than the intended corpus (64 MiB) or label-suite (2 MiB) limit before detecting overflow. Files without a reliable regular-file size, such as FIFOs, also lacked an in-flight read bound.

## Implementation

`read_bounded` now opens the path exactly once, checks `File::metadata` on that handle, and reads through `Read::take(max.saturating_add(1))`. It accepts at most `max` bytes, reading one additional byte only to detect overflow, then returns an error if the input exceeds its configured limit. The same descriptor is used for the preflight and read, so replacing the pathname cannot redirect the reader to a different file.

This applies to caller-provided `--corpus` and `--suite` input. Pack and immutable-snapshot decoding have their own bounded verification paths. This change does not claim that files remain unchanged after reading, that input is publisher-authenticated, or that labels are correct.

## Tests and exact-commit CI

Added `bounded_input_reader_accepts_exact_limit_and_rejects_overflow`: it exercises exact-size acceptance, one-byte-over rejection, an empty file with a zero-byte limit, and nonempty input with a zero-byte limit. The in-flight `Read::take` bound also covers files that grow while being read; concurrent mutation was not separately stress-tested.

- First implementation SHA `9ccc01f58d7ad3edff18a0c1d585802d2fe4e6f1` failed rustfmt ([Rust workspace 41379504920](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41379504920)).
- Intermediate formatting SHA `e44da7ed34ad55316ff3d5d1b9adc1aeb87eab94` still failed rustfmt ([Rust workspace 41379556607](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41379556607)).
- Final source SHA **`3aefff254fe5425fa87de0c30ff2341ffd0eeaf6`** passed [Rust workspace 41379617324](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41379617324), [Platform shells 41379617292](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41379617292), and [Android Rust ABI qualification 41379617258](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41379617258).

## Remaining acceptance

RAG-003 requires a representative independently labeled corpus, authenticated Wikimedia source and label provenance, retrieval quality thresholds on real devices, and factual-answer entailment evaluation. The desktop AI chat UI remains read-only and does not yet load canonical evidence or invoke the shared consent-gated inference use case.
