# RAG-003 — Atomic, no-clobber retrieval benchmark report publication

**Date:** 2026-11-01 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `c49ce399899a08778713ea3ed26083ae44f46b53`  
**Status:** Partial. RAG-003 remains unchecked.

## Problem

The offline retrieval suite runner verified that the requested report destination did not alias its corpus or label suite, but subsequently wrote JSON using `fs::write`. This truncates an existing report, and an attacker or competing process could replace the target with a symlink after preflight. Failed writes could also expose an incomplete JSON report.

## Implementation

- Preflight rejects an already-existing destination, including dangling symlinks. Every run requires a new output filename; callers must not silently overwrite earlier benchmark evidence.
- `persist_new_report` creates a unique temporary file in the destination directory with `create_new`, writes the complete JSON and calls `sync_all` before publication.
- `hard_link` publishes the complete file at the final name without replacement. If another process creates the destination between validation and publication, the link fails instead of overwriting that file or following its symlink.
- The temporary name is removed after success or error. An already-published report is left intact on a conflicting second invocation.

This is an atomic visibility and no-clobber property for a report file on a filesystem supporting hard links. It is not a cross-platform power-loss durability guarantee for the containing directory, nor a defense against an adversary able to replace the parent directory while the process is running.

## Tests and CI

The example's regression tests now verify that an existing non-input report is rejected during preflight, successful report publication produces the exact JSON content, a competing second publication cannot replace it, no staging files remain, and an output symlink cannot alter its source file.

- First source commit `89c6598559816d5eaae6023c667cb50b3f94fc5b` failed Rust formatting: [Rust workspace 41374482582](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41374482582).
- Corrected source commit **`c49ce399899a08778713ea3ed26083ae44f46b53`** passed [Rust workspace 41374545493](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41374545493), [Platform shells 41374545546](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41374545546) and [Android Rust ABI 41374545551](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41374545551).

## Remaining

RAG-003 still requires independently labeled representative questions, publisher-authenticated corpus provenance, measured retrieval performance on supported hardware, and factual-answer entailment evaluation. The desktop chat surface still lacks a canonical article loader and inference UI. No production or real-device acceptance is claimed by this isolated report-integrity improvement.
