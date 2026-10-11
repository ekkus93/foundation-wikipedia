# SRC-002 / SEC-001 — Canonical reference URL safety (partial)

Qualified implementation: `bb884116888bcf5e718419b424c845312199c3d3`.

- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38113331789 — passed at the exact implementation SHA.
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38113331602 — passed at the exact implementation SHA.
- `wiki-source::enterprise_normalize_references` now validates canonical reference URLs before persisting them. It accepts absolute HTTP(S) links with hosts and no userinfo; rejects executable, file/data, malformed, whitespace/control-bearing, credential-bearing, overlong and backslash-ambiguous links.
- Negative/positive regression tests live in the module. Earlier attempt to move conflicting citation-marker detection into the upstream evidence preflight changed an established `InvalidReference` error contract; this was reverted in `e3624d6d78f4bfc9b63c170d2e5924c7ccfd95cb`.

**Not full task acceptance:** SRC-002 remains unchecked pending authoritative Wikimedia generation fixture coverage, complete required visual/media joins, rendered HTML fidelity and source completeness. SEC-001 remains unchecked pending hostile input/reader/bridge review and cross-platform acceptance. This document is supplemental qualification evidence; the canonical TODO could not be updated because the large-file write was rejected by the tool safety gate.
