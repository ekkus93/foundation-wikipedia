# RAG-003 — Verified Provisional .wpack Corpus Benchmark Input (2026-10-24)

## Implementation

The existing `crates/wiki-search/examples/retrieval_suite_runner.rs` can now evaluate an article array stored as the `articles.json` object of a provisional `.wpack` envelope, in addition to the existing raw `--corpus` JSON input. Exactly one input option is required.

```sh
cargo run -p wiki-search --example retrieval_suite_runner --locked -- \
  --pack corpus.wpack --suite labels.json \
  --sha <exact-40-character-commit> --output report.json \
  --top-k 5 --min-recall-per-mille 800 --max-no-answer-fp 0
```

The pack reader uses `wiki-pack-format::container::verify_container` to stream-verify the **entire** envelope, including object hashes and the absence of trailing bytes, before reading any article object. It computes the canonical offset of the `articles.json` object, enforces the 64 MiB corpus bound, seeks on the same open handle, and independently rechecks the selected object SHA-256 after reading. It does not extract media or write any untrusted object to disk. Missing article objects and malformed/corrupted envelopes fail closed.

The existing suite parser still verifies gold block identities against the exact indexed revision. Unit tests cover the verified multi-object pack, corruption of an unrelated media object, appended bytes, missing article arrays, and invalid simultaneous CLI input selection.

## Qualification and scope

Implementation commit `583a33fc6ee0296db8d6af69d3189476782e13d7` initially failed rustfmt. Formatting correction `279f3b769e15faedf468b74c157989a3a052dcb4` passed format but required the `wiki-pack-format` dependency entry in `Cargo.lock`. The corrected source plus lockfile is `88619556ca23346e9f334cc6e3d5fce49156ab14`.

This is **not** general production pack ingestion. It supports the deliberately narrow provisional pack convention of one canonical `articles.json` array. It does not decode production CBOR/zstd shards, prove that the pack's articles are complete, verify an official publisher signature, authenticate the evaluation labels, or measure device latency. The Git commit in the report is caller-asserted. The input pack must be controlled by the caller while reading; rehashing the selected object mitigates but does not eliminate concurrent file replacement risks.

**RAG-003 remains unchecked.** Next steps: a stable production article-object layout and decoder, authenticated Wikimedia corpus and independently reviewed relevance labels, production pack CI baselines, answer-support grading and device measurements.
