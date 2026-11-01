# AI-004 — Bind saved profiles to exact discovery and confirm deletion

**Date:** 2026-11-01  
**Status:** Partial AI-002/AI-004; parent tasks remain unchecked.  
**Scope:** Desktop local Ollama/OpenAI-compatible profile selection and deletion only.

## Failure modes and implementation

A selected model could previously reach the desktop Save handler based only on the presence of an inventory object and nonempty model string. Ordinary UI edits clear the inventory, but the final save path did not independently bind the selected ID to the exact discovered protocol, endpoint and locality. `canSaveAiLocalChoice` now requires all of those to match and the model ID to be in the advertised inventory. The Save button and handler enforce the same guard. The native Rust endpoint and SQLite validators remain authoritative; saved models remain unverified hints until the next explicit discovery.

A saved local profile could also be deleted with a single click. The desktop now requires a separate inline confirmation that names the selected profile and explains that deletion removes only its saved configuration. A name/endpoint edit, profile switch, saved-state refresh or conflict-recovery read clears pending confirmation. Confirm is blocked while a newer saved profile snapshot requires review. Native revision-aware compare-and-swap still rejects stale mutations.

Neither change initiates inference, remote/cloud requests, model discovery, or removal of installed models.

## Regression evidence

`desktop/scripts/aiLocalModels.test.mjs` covers valid exact model binding; absent/stale/foreign inventory, unadvertised/spoofed model IDs, endpoint/protocol/locality changes; missing or mismatched delete confirmation; absent profile and conflict-review fences.

**Exact discovery/save code:** `6f4f920c7a9fdc5c4b94185e2bf8ff96c6630981`
- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41286032586 — passed.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41286032503 — passed.
- Desktop launch smoke: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41286032505 — passed.

**Exact deletion-confirmation code:** `6c52997e52038191c70106b5b31a148aaa5136f9`
- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41286324586 — passed.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41286324602 — passed on attempt 2.
- Desktop launch smoke: https://github.com/ekkus93/foundation-wikipedia/actions/runs/41286324626 — passed on attempt 2.

The first desktop dependency-install attempts failed with npm registry HTTP 404 for `rollup-4.67.4.tgz`. The same exact commit passed after rerunning failed jobs; no dependency or source changes were required to repair those external failures.

## Remaining qualification

Real simultaneous-window desktop acceptance, live Ollama/llama-server LAN verification, remote-provider credentials and explicit passage consent, article-grounded inference with valid citations, and physical desktop/Android acceptance remain outstanding. This does not justify checking AI-002, AI-003, AI-004 or AI-005.
