# RAG-002 — strict citation marker parsing (partial)

- Implementation: `crates/wiki-ai/src/rag.rs`.
- Exact implementation SHA: `869dbb92e8eddc97643163ef66376ddc630936e3`.
- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38077633181 — success.
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38077633133 — success.
- Android Rust ABI qualification: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38077633198 — success.
- Behavior: citation-like tokens with missing colons, unterminated delimiters, empty IDs, whitespace or nested brackets now fail before a model answer is returned as grounded. Well-formed exact evidence IDs remain eligible for existing retrieved-block validation.
- Remaining: factual support evaluation, real provider integration, citation highlight/navigation, source and snapshot lifecycle. RAG-002 remains unchecked in the canonical TODO.
- This file is supplemental qualification evidence, not a replacement checklist.
