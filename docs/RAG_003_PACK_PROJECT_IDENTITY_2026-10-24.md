# RAG-003 — Provisional Pack Project/Article Identity (2026-10-24)

The offline retrieval benchmark runner now binds the verified provisional `.wpack` manifest's `project` to **every** decoded canonical article key. An intact object SHA-256 and container envelope do not by themselves prove that the article array belongs to the Wikimedia edition advertised by the pack. Previously, a pack declaring `enwiki` could carry an `articles.json` array with `frwiki` article keys and pass the envelope verifier.

`read_pack_corpus` returns the verified manifest project alongside the bounded article JSON. After deserialization and before building the lexical index or writing an evaluation report, the runner rejects any cross-project article. This applies only to `--pack`; the raw `--corpus` mode intentionally permits multi-project evaluation suites. Regression tests cover matching and mixed project keys and the manifest project returned by the verified pack reader.

This is a **local identity-consistency guard**, not publisher authentication or independent completeness verification. It does not add production CBOR/zstd shard decoding, provenance-qualified Wikimedia evaluation labels, factual answer-support grading, or physical-device performance measurements. **RAG-003 remains unchecked.**

## Exact-head CI qualification

Code and scoped regression tests at [`a973c9b431480d2337697c28f1bb65ffa831ab83`](https://github.com/ekkus93/foundation-wikipedia/commit/a973c9b431480d2337697c28f1bb65ffa831ab83) passed [Rust workspace #40117245204](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40117245204), [Platform shells #40117244927](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40117244927), and [Android Rust ABI #40117245331](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40117245331). These are automated build/test results, not physical-device acceptance.
