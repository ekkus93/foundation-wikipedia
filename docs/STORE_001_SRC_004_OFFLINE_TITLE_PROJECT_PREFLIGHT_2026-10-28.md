# STORE-001 / SRC-004 — Offline title project preflight (2026-10-28)

## Defect and correction

The public SQLite catalog `SnapshotCatalog::lookup_key` previously accepted any project string. A caller could request an empty, uppercase or slash-containing edition and receive `None`, which is reserved for genuinely absent titles in a valid edition. Redirect-aware `resolve_title` inherits this behavior because it delegates to `lookup_key`.

The catalog now validates a synthetic `ArticleKey { project, page_id: 1 }` using the canonical model contract before looking up the normalized title. Invalid project IDs return `CatalogError::Model(ModelError::InvalidIdentity)` without a SQLite query. A valid but uninstalled edition still returns `None`.

## Synthetic verification and exact-head CI

The existing disk-backed read-only catalog regression verifies normal `enwiki` title lookup, valid missing `frwiki` lookup and invalid empty/uppercase/slash-containing project strings through both direct and redirect-aware title lookup. Source commit `b8e21f0072be198adcd97075fca97b3be005e3dc` triggered [Rust workspace 40808541703](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40808541703), [Platform shells 40808541705](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40808541705), [Android ABI 40808541755](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40808541755) (passed), and [Android codec probe 40808541737](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40808541737). The other workflows were pending when this document was written; no pending check is counted as passed.

## Scope

This validates a caller-supplied project ID, not authenticated Wikimedia source completeness, mandatory offline media and attribution, signed pack trust, activation, production desktop/Android integration or device acceptance. **STORE-001 and SRC-004 remain unchecked.**
