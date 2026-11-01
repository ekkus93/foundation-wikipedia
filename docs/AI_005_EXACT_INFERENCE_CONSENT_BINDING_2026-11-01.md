# AI-005 / RAG-002 — Exact inference-settings consent binding

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Source SHA:** `c801327d106705138ccb77cdb3c6352b01ef1005`  
**Scope:** Shared Rust `wiki-ai`; partial qualification, not full chat acceptance.

## Problem

The existing article-passage preview compared the exact selected endpoint/model, article-revision prompt and transmitted evidence before inference, but it did not bind consent to output-token limits, outbound locality policy, retrieval budgets or whether evidence was explicitly selected. Changing those controls after disclosure could still reuse a preview if the resulting evidence bytes happened to match. In addition, the preview execution path rebuilt the same article lexical index a third time after successfully verifying it.

## Implementation

`LocalArticlePreview` now privately records the exact question text, passage count, word budget, selected evidence, output-token cap and outbound policy along with its existing selected endpoint/model and prepared context. `answer_article_with_preview` rejects changed settings with `PreviewMismatch` **before contacting any provider**, even when the selected passages would be identical. The caller still must provide affirmative per-request `share_article_passages` consent; the preview itself never authorizes traffic.

The shared `rag::answer_article` now delegates to a private `answer_prepared_article` helper. After validating the current article, question and prepared prompt against the disclosed preview, the preview-specific execution path passes that verified prepared context directly to the helper. This avoids a redundant third lexical-index build and ensures the transmitted prompt is the exact context checked against consent. The helper remains private to `wiki-ai` and does not allow arbitrary external callers to bypass canonical article validation.

## Verification

The existing loopback Ollama GET/POST/SSE test now exercises changed output-token cap, outbound locality policy, passage limit, word budget and selected evidence, asserting each is rejected as `PreviewMismatch` before the two expected inference POST requests. It also retains negative checks for changed article revision, question, endpoint and absent consent; the unchanged preview produces a citation-validated answer. The ordinary no-preview RAG API and its existing regression suite continue to use the same prepared-context execution helper.

All workflows passed for the exact source SHA:

- [Rust workspace — 41341034202](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41341034202)
- [Platform shells — 41341034197](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41341034197)
- [Android Rust ABI qualification — 41341034366](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41341034366)

## Remaining

The desktop chat panel is still read-only and does not expose canonical article evidence, show the exact preview and destination, obtain passage-sharing consent, start new inference, persist validated answers or navigate to verified citation blocks. Android integration, live local-model LAN acceptance, factual-entailment evaluation and physical-device tests remain outstanding. **AI-005 and RAG-002 remain unchecked.**
