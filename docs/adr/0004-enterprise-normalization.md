# ADR — Provisional Wikimedia Enterprise canonical normalization

**Status:** Partial implementation; SRC-002 remains unchecked (2026-10-10).

The source adapter joins Structured Contents and regular Enterprise article
components by exact project, page ID, revision, modification timestamp,
namespace, language, title, and generation. A verified NDJSON import now
produces a canonical `wiki_model::Article` or rejects the entire batch.
The source generation ID and category membership remain in the typed
`CanonicalEnterpriseArticle` wrapper, not in the article's HTML.

The article's content digest is SHA-256 of deterministic JSON serialization
with the digest field replaced by 64 zero characters. It covers structured
blocks, reference records, retained HTML, identity and revision fields.
The importer maps paragraphs, nested sections, tables, simple lists,
infobox fields and references; unsupported beta shapes fail closed.

**Not qualified for complete offline reading:** Wikimedia image/Commons
metadata, hashes, licenses and required assets are not yet joined.
An article containing unresolved structured images or HTML image elements is
rejected rather than incorrectly labeled offline-ready. Rendered HTML is
retained verbatim and **must be sanitized before any WebView renders it**.
Link-to-page-ID resolution, rich footnotes, disambiguation metadata, complete
visual fidelity, real official-snapshot regression fixtures, and source
availability/hosting evaluation remain outstanding.

Qualification uses `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo test --workspace --locked`, and exact-head GitHub Actions.
Do not mark SRC-002 complete until its acceptance criteria and relevant
visual/HTML fidelity checks are evidenced.
