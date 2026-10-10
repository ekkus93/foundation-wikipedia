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
metadata, hashes, licenses and required assets are not yet joined. Canonical
normalization sanitizes retained HTML at the source boundary with Ammonia,
removing active script/event-handler/unsafe-URL content. Before sanitization,
the pipeline rejects unresolved image/picture/video/audio/source/track,
SVG/MathML/canvas/iframe/object/embed markup so required offline semantics
cannot be silently stripped and mislabeled as complete. Disambiguation
metadata, complete visual fidelity, representative real official-snapshot
regression fixtures, and source availability/hosting evaluation remain
outstanding.

Qualification uses `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo test --workspace --locked`, and exact-head GitHub Actions.
Do not mark SRC-002 complete until its acceptance criteria and relevant
visual/HTML fidelity checks are evidenced.

**Citation preservation increment:** Structured Contents citation IDs that
resolve to same-revision references are now represented as canonical
`Footnote` blocks, with a revision-local source ID, upstream marker text,
reference title and explicit reference linkage. Duplicate uses of the same
reference ID emit one footnote; conflicting marker text fails closed.
Unresolved Structured Contents page links are rejected rather than silently
omitted until a verified page-ID resolver is available. This does not
constitute complete reference/footnote rendering fidelity or safe HTML.

**Exact-generation link resolution increment:** Canonical normalization uses a
deterministic page-title/redirect index bound to one verified project and
generation and resolves supported Wikipedia article links to canonical
project/page IDs while retaining URL fragments. A snapshot-wide identity pass
can build this index once and reuse it across bounded normalization chunks, so
a target does not need to be present in the current chunk. Percent-encoded
Unicode titles and underscores are decoded; unknown targets, ambiguous titles,
mixed project/generation identities, duplicate page IDs, cross-project/external
links and revision-query URLs fail closed. Standalone normalization
intentionally rejects unresolved links. Redirect graph validation, HTML link
interception, verified media joins and real-snapshot fidelity remain pending.
