# RAG-003 — Offline retrieval runner argument and output safety

**Date:** 2026-10-24  
**Status:** Scoped implementation qualified; RAG-003 remains incomplete  
**Qualified code commit:** `d0cb1b303c826a00a3eba6df19264307185a4769`  
**Path:** `crates/wiki-search/examples/retrieval_suite_runner.rs`

## Change

The provisional offline benchmark runner rejects duplicate command-line flags, including `--top-k`, `--min-recall-per-mille`, and `--max-no-answer-fp`. Previously a repeated threshold could silently replace the earlier value, making the effective benchmark policy ambiguous.

Before opening a corpus or writing a report, the runner resolves the output destination and both input paths. It refuses to overwrite its own input corpus, input pack, or labeled suite, including equivalent paths with `./` and existing output symlinks. It also rejects dangling output symlinks. This protects the local evidence and evaluation labels from accidental destruction when the operator supplies `--output` incorrectly.

## Verification

- Rust workspace exact-head CI: [40121425604](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40121425604) — passed.
- Platform shells exact-head CI: [40121425660](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40121425660) — passed.
- Android Rust ABI exact-head CI: [40121425606](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40121425606) — passed.
- Regression tests cover duplicate threshold flags, noncolliding report paths, collisions with both input files, and a Unix symlink to the suite.
- An initial commit `f79a00f63789f624611f98371b572277d34565a4` failed rustfmt; follow-up formatting and restoration of a truncated test tail yielded the qualified source commit above.

## Scope and remaining requirements

This is a local CLI safety improvement, not a claim of secure concurrent filesystem access or atomic report persistence. The runner still evaluates caller-supplied labels and provisional canonical `articles.json` pack data; it does not independently authenticate Wikimedia provenance, decode production CBOR/zstd shards, grade factual answer support, or provide representative device measurements. **RAG-003 remains unchecked.**
