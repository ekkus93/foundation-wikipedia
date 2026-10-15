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
