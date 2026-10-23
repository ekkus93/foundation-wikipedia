# SRC-003 — Linear-time XML tag quote-state guard (2026-10-23)

**Scope:** Narrow performance and safety qualification of the portable public-dump XML envelope validator; this is not completion of SRC-003.

## Defect and correction

The streaming `XmlNestingGuard` buffered at most 64 KiB of markup, but on every literal `>` it scanned the entire buffered tag to discover whether that delimiter was inside a quoted attribute. An adversarial or malformed near-limit tag containing many `>` characters inside an attribute could therefore trigger quadratic CPU work despite the byte-size bound.

The validator now tracks the current single/double attribute quote in `tag_quote` as each byte arrives. A `>` ends a tag only when no quote is open. Quote state resets on entry to a new tag. All other token length, XML character, entity, nesting and poison-state checks remain in force. Tag scanning is linear in input length, with bounded tag buffering.

## Evidence

- **Source:** `752684547c65bb44cec92c7734dec5852cfd4810` on `master`.
- **Rust workspace CI:** https://github.com/ekkus93/foundation-wikipedia/actions/runs/40034744026 — success, including rustfmt, strict Clippy and workspace tests.
- **Platform shells CI:** https://github.com/ekkus93/foundation-wikipedia/actions/runs/40034744283 — success.
- **Regression:** `quoted_greater_than_is_streamed_in_linear_time` exercises 60,000 quoted `>` characters (within the 64 KiB tag cap), both quote styles, four write chunk sizes (1, 2, 7, 8192), and rejection of unterminated quotes.

The regression exercises the worst-case input shape but is not a formal CPU benchmark. The change removes the rescanning loop by construction.

## Remaining acceptance

This does not authenticate the Wikimedia publication, independently establish source census completeness, prove exact-revision HTML rendering, include required offline images/math/attribution, or qualify a physical Android or desktop reader. Keep **SRC-003 unchecked** until the full acceptance criteria are met.
