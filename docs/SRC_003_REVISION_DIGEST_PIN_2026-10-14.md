# SRC-003 diagnostic revision digest pin

The `wiki-pack audit-diagnostic` command accepts an optional seventh argument containing a SHA-256 digest of the revision-content JSONL input. The existing six-argument command is preserved. The new check hashes the exact bytes consumed by the streaming Rust auditor and rejects a valid but altered revision record even when the expected page IDs are unchanged.

Qualified implementation SHA: `9b5b08f26d2e3cb5f018b52f9af6958bd55c72f8`.
Rust workspace CI: run 38686480198 (passed). Platform shells CI: run 38686480217 (passed). Tests include valid, malformed and mismatched digest pins and backward compatibility.

This integrity check does not authenticate Wikimedia, prove independent census completeness, render revision-matched HTML, or qualify offline media and packs. SRC-003 remains unchecked. The canonical TODO and existing census-pipeline documentation are unchanged.
