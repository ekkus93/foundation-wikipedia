# RAG-003 — Stable, nonempty evidence-file verification

**Date:** 2026-11-03  
**Status:** Partial; RAG-003 remains unchecked.

## Scope

The strict `--evidence-root` manifest verifier previously accepted an empty
artifact if its manifest supplied the SHA-256 digest of empty bytes, and
accepted the digest of bytes read from a file that was modified while hashing.
The former does not constitute a meaningful plan, reviewed corpus, or report;
the latter can leave a stale receipt for a changing evidence bundle.

The verifier now rejects **zero-byte** artifacts even with a matching digest.
After reading a single opened file descriptor, it also compares the number of
bytes read with the initial file size and rechecks device, inode, size, and
nanosecond modification/change timestamps. Ordinary append, truncation, and
in-place rewriting during hashing fail closed instead of producing a successful
local SHA-256 receipt.

Deterministic synthetic tests cover an empty report with the correct empty
digest and a concurrent append triggered after the verifier has hashed the
original plan bytes. The existing positive distinct-file and negative
hardlink/path-alias tests are retained.

## Qualification limits

These checks detect ordinary in-flight mutation; they do **not** lock files,
eliminate every hostile filesystem race, validate the actual JSON semantics of
the reviewed corpus/report, prove preregistration timing, authenticate Wikimedia
publication or reviewer independence, or establish factual entailment.
Independently authenticated evidence and real review remain required by
`docs/RAG_003_INDEPENDENT_QUALIFICATION_PROTOCOL_2026-11-03.md`.
