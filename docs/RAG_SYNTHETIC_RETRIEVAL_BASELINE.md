# RAG synthetic lexical retrieval regression baseline

This is **partial RAG-003 evidence only**. The synthetic dataset in
`crates/wiki-search/tests/labeled_retrieval.rs` includes six labeled
single-block queries, one cross-section query, Unicode, an explicitly
unanswerable query, and an exact-revision mismatch.

The test requires **6/6 labeled block recall at top 3** on the synthetic
corpus and explicit conservative behavior for unrelated queries, zero budget
and changed revisions. Run with:

```sh
cargo test -p wiki-search --locked --test labeled_retrieval
```

This is a deterministic regression fixture, not a representative Wikipedia
benchmark. It does **not** measure generated-answer factual entailment,
answer support, citation precision, production pack search latency, or mobile
RAM/storage. RAG-003 remains unchecked until sourced question fixtures,
broader measured metrics and real end-to-end evaluations are qualified.
