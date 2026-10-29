# STORE-001 / RAG-002 — Offline page identity preflight (2026-10-28)

## Defect and correction

The shared local-only `OfflineReader::open_page` previously accepted a caller-supplied `ArticleKey` without validating its project/page identity. A malformed project or page ID zero could therefore be indistinguishable from a valid uninstalled page (`Ok(None)`). Saved exact-revision and redirect-target citation paths delegate to this lookup, so they inherited the ambiguity for otherwise valid revision/target arguments.

The entry point now calls the canonical `ArticleKey::validate` before catalog access and returns `ReaderError::Catalog(CatalogError::InvalidCatalogEntry)` on malformed identity. Valid missing pages still return `None`; valid installed pages and redirects retain their existing behavior. No network, snapshot mutation or trust promotion occurs.

## Regression and CI scope

The existing on-disk two-record snapshot/SQLite/FTS fixture exercises a valid missing page and five invalid keys (zero, overflow, empty project, uppercase project and slash-containing project) through `open_page`, `open_exact_revision` and `open_exact_page_target_revision`. The initial source SHA `9561dfb01feff5f8b9ad899c88a2963b0a900912` failed formatting ([Rust run 40807838111](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40807838111)). Corrected source SHA `8988e2c73923b25e13909124365b12302e9ff4f9` has [Rust run 40807913353](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40807913353), [Platform run 40807913282](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40807913282), [Android ABI run 40807913220](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40807913220) (passed), and [Android codec probe 40807913321](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40807913321); other runs were still pending at this report's creation. Their final status must be checked on the exact source SHA.

## Boundaries

This qualifies only the page-key input boundary, not publisher authentication, mandatory visual/media and attribution completeness, pack activation, production Tauri/Android UI wiring or real-device acceptance. **STORE-001 and RAG-002 remain unchecked.**
