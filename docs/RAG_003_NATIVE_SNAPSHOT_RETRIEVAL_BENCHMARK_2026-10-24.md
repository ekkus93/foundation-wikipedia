# RAG-003 — verified native snapshot retrieval benchmark

**Date:** 2026-10-24  
**Status:** Scoped implementation and synthetic CI qualification; RAG-003 remains unchecked.

## New benchmark source

The `wiki-search` example `retrieval_suite_runner` accepts a third mutually
exclusive corpus mode:

```sh
cargo run -p wiki-search --example retrieval_suite_runner --locked -- \
  --snapshot-root /path/to/immutable-snapshot \
  --suite /path/to/labeled-evaluation-suite.json \
  --sha <exact-40-hex-source-commit> --output /path/to/report.json \
  --top-k 5 --min-recall-per-mille 800 --max-no-answer-fp 0
```

The snapshot root must contain `catalog.sqlite` plus the catalog-referenced
shard files. The runner opens SQLite read-only via
`wiki_store::catalog::SnapshotCatalog::open_read_only`, checks complete shard
coverage and title index consistency via `verify_shard_coverage`, then
iterates and re-verifies each individually digest-checked MessagePack/zstd
record via `visit_records`. Redirects are inspected for integrity but not
indexed as separate article passages; only canonical `Article` records are
indexed. No source files are modified and reports within the snapshot tree
are refused. Existing `--corpus` JSON and provisional `--pack` modes remain.

Resource policy: no more than 20,000 canonical articles enter the in-memory
lexical corpus. The underlying snapshot iterator audits compressed frames
individually rather than decompressing an entire snapshot shard.

## Verification

- First implementation `90657903cbe77da8cc3552c812995db8f7c6935c`
  failed only rustfmt ([Rust CI 40123841730](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40123841730)).
- Corrected implementation `a1eb636b0aab351fcdf67ad7ad4b1236487d3444`
  passed [Rust 40123922147](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40123922147),
  [Platform shells 40123922064](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40123922064)
  and [Android Rust ABI 40123922215](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40123922215).
- Follow-up `dc53147648c3863180d08f238278b8542256a30d` added a
  complete on-disk snapshot/label-suite/report regression.
- Source SHA `2662420a10180d00b41f510855f4b7daf09c840a` makes
  GitHub Actions run `cargo test -p wiki-search --example retrieval_suite_runner --locked`
  to execute example integration tests on every Rust CI run, including the
  zstd/SQLite snapshot test. [Rust workspace 40124223144](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40124223144)
  **passed**; [Platform shells 40124223138](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40124223138)
  was still running when this evidence was recorded.

The test constructs a genuine on-disk SQLite catalog and compressed record
frame, confirms a labeled revision-scoped suite produces a passing JSON
report, then tampers with the frame and requires import rejection. It also
rejects a report destination inside the snapshot directory.

## Limits and outstanding RAG-003 acceptance

These checks establish locally consistent byte/frame/catalog identities,
not independently authenticated Wikimedia source publication, complete
category coverage or required offline media and licensing. A source commit
SHA and gold labels remain caller supplied; the benchmark does not prove
that they describe a real Wikimedia publication. The lexical relevance suite
does not grade factual answer entailment. Production-pack and physical-device
performance measurements still require separately qualified sources,
representative labels and repeatable benchmark executions.
**Do not mark RAG-003 complete.**
