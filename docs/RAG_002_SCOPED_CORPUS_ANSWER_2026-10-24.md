# RAG-002 — Explicit scope for multi-article grounded answers

**Date:** 2026-10-24  
**Scope:** Partial RAG-002 implementation, not production acceptance.

## Behavior

Added `wiki-search::validate_scoped_corpus_answer`, which requires the
application to pass an explicit allowlist of article keys before a
multi-article answer can be labeled supported. Each allowlisted article
must exist in the current in-memory corpus index. All retrieved passages
and model-claimed citations must belong to the allowlist, then pass
existing project/page/revision/block identity and exact-supplied-evidence
validation. An empty or unknown scope, out-of-scope page, fabricated or
unretrieved citation, duplicate citation, tampered/stale evidence,
uncited nonempty answer, and hidden text in an abstention fail closed.

The API returns a typed supported answer with verified evidence or a
typed insufficient-evidence abstention. **It cannot establish that the
user consented to sharing multiple articles with an LLM.** That consent
must be obtained and enforced by the calling UI/use case *before*
constructing a model prompt. It also does not prove generated factual
claims are entailed by the retrieved passages.

## Regression and CI

- Initial implementation `0213918a60bdcba94de73cf582b8b55ebd9d2da8`
  failed rustfmt only in [Rust workspace 40079512501](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079512501).
- Corrected implementation SHA:
  `bb9bec51b724c96e826f49e509e361644c298be0`.
- [Rust workspace 40079562794](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079562794): passed.
- [Platform shells 40079562818](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079562818): passed.
- [Android Rust ABI 40079562743](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40079562743): passed.
- Tests cover authorized two-page answers, empty/unknown allowlists,
  unapproved retrieved/claimed pages, uncited answers, explicit abstention,
  unretrieved second-page claims and tampered retrieved text.

## Outstanding

The public function is not yet wired to the production desktop/Android
chat use cases, user consent controls, persisted source snapshots,
pack/media verification, or physical-device acceptance. Factual
entailment evaluation and prompt-injection robustness remain separate
requirements. **RAG-002 remains unchecked.**
