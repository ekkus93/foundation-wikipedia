# SRC-003 — bounded source text lines for diagnostic XML (2026-10-15)

**Status:** Narrow diagnostic-reader memory guard only. SRC-003 remains
unchecked; this is not a production Rust XML/bzip2 importer.

The existing Python public-dump fixture runner iterated over the decoded
text stream using the TextIO line iterator. That can allocate an unbounded
string before the downstream page/header guards have an opportunity to
reject it. The importer now passes a `_bounded_lines` iterator into the
existing filter, reading at most 64 MiB plus one Unicode character per
`readline(size)` call and rejecting a line that exceeds the cap.
The per-page 64-MiB **UTF-8 byte** guard and independent 8-MiB header
guard remain separate; character and byte counts are deliberately not
interchanged. A malformed, very large single-line current-content export
fails closed rather than producing a successful diagnostic render.

Regression tests reject a long single input line, invalid bounds and
unbounded `readline()` usage, and permit normal short lines. The generic
`filter_dump` function continues to accept iterable fixture lines, so
direct callers must also bound their own source reads.

This has no bearing on source publication signing, independent upstream
census, template/module revision stability, Commons media acquisition or
offline-ready pack fidelity.

**Qualification:** inspect Rust, Platform and renderer exact-head CI before
calling this narrow memory guard successful.

## Qualified implementation and exact-head CI

The bounded decoded XML line reader at `057d1b75dd3ced3f9583c1f86e31b63d126a06f5` was accepted on master by Ralph Bridge operation `fd532263130a4666a0698bdbb6b80ac5`, `write_reconciled=false`. [Rust 38825732884](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825732884), [Platform 38825732814](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825732814) and [Renderer 38825732820](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825732820) all passed at the exact same source SHA. This establishes only the diagnostic reader's line-cap behavior.
