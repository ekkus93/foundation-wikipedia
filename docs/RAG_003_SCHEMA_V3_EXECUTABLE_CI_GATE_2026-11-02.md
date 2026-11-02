# RAG-003 — Schema-v3 executable answer-support CI qualification

**Date:** 2026-11-02  
**Passing source commit:** `ad7095e34cf5964c902ef146481b6ff73086e361`  
**Status:** Partial; RAG-003 remains unchecked.

## Problem

The Rust workspace workflow ran `cargo test --workspace --locked` and explicitly tested the retrieval-suite example, but it did not run the `answer_support_runner` example's unit tests. The newly added schema-v3 complete-visible-prose policy therefore lacked a CI-executed end-to-end strict-corpus regression. Adding that command immediately revealed a latent compilation failure in the example tests: `Args` was used with `.clone()` without implementing `Clone`.

## Implementation

- Added `cargo test -p wiki-search --example answer_support_runner --locked` to the exact-head Rust workflow's piped test log, under `set -o pipefail`. A failed example test now fails the workflow and retains its diagnostic test log.
- Corrected the example-test `Args` type to derive `Clone`, so existing example tests actually compile and execute.
- Upgraded the executable strict canonical-corpus regression to use reviewed-answer annotation **schema v3** with exact UTF-8 claim spans, retaining positive tests for canonical citation membership, paired SHA-256 input pins, 100% aggregate and per-case quality floors, and no-clobber report publication.
- Added a negative strict-corpus regression: a valid reviewed claim and canonical citation followed by **unreviewed visible prose** must fail before a benchmark report is published.

## Verification

Initial implementation `fafaf3be126a2e95702736d118e08b492158bee6` failed Rust workspace CI as expected after the newly executed example tests exposed the missing `Clone` implementation ([run 41495184161](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41495184161)). The corrected source commit `ad7095e34cf5964c902ef146481b6ff73086e361` passed all three workflows:

- [Rust workspace — run 41495576805](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41495576805)
- [Platform shells — run 41495576753](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41495576753)
- [Android Rust ABI qualification — run 41495576949](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41495576949)

## Qualification limits

This is **synthetic test plumbing** with a caller-created canonical article, not an independently authenticated Wikimedia publication or independently labeled representative answer corpus. Schema-v3 word-span coverage and human verdict bookkeeping do not prove that every distinct factual proposition is separately reviewed or entailed by the cited evidence. Real-device testing, source authentication and representative independent review remain outstanding; keep RAG-003 unchecked.
