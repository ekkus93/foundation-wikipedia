# SRC-003 — Diagnostic XML root and header boundary (2026-10-15)

**Scope:** fail-closed public-dump renderer fixture filtering; not production
Wikimedia import and not an offline-pack release gate.

The public-dump diagnostic filter previously accepted arbitrary further input
after the first closing `</mediawiki>`, and it wrote the initial XML
site-information header without an independent bound. An attacker-controlled
or malformed descriptor could append a second page/root or an oversized header
while the importer still returned a successful sample. The updated filter
rejects all additional lines after the closing root, duplicate opening root
markers, and a pre-page XML header exceeding 8 MiB. The header cap is
configurable for deterministic tests; invalid limits reject before iteration.

The existing per-page 64 MiB diagnostic bound remains in place. These checks
are deliberate strictness for export-0.11 pretty-printed input, not a generic
namespace-aware XML parser. They do **not** independently authenticate the
source publication or complete page census, acquire Commons media, verify
license/attribution, or ensure offline-renderable packs. The canonical SRC-003
and PACK-003 tasks remain unchecked.

**Qualification:** exact-head Rust workflow (which runs Python fixture tests),
Platform workflow and real-dump renderer workflow must be inspected before
claiming this source slice passed.

## Closing-page guard refinement

The main loop now checks for an already-closed root *before* examining any
kind of XML line. A stray closing `</page>` after the root is therefore a
controlled `DumpFormatError` rather than attempting to append to a missing
page buffer. A stray closing page tag between legitimate pages is also
reported as an explicit unmatched-page error. Both are covered by regression
tests. This is input diagnostics, not source publication completeness.

## Qualified implementation and exact-head CI

The root/header guards were committed at `9e9e5f5b49ee17274823244464fe0d3648fe2a25` (Ralph `commit_files` operation `dc179286957147b49f52255b54514198`, `accepted=true`, `write_reconciled=false`) and passed [Rust 38825625430](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825625430), [Platform 38825625463](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825625463), and [Renderer 38825625462](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825625462) at that SHA. Unmatched-closing-element rejection was committed at `6b3232308b3cdb7d462c2ddbbb1d42c4abe8f331` (operation `fcdcac4b34fc4d82ae29efc53b3010ff`, accepted and no reconciliation ambiguity) and passed [Rust 38825854699](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825854699), [Platform 38825854723](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825854723) and [Renderer 38825854704](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38825854704). These do not qualify SRC-003 completion.
