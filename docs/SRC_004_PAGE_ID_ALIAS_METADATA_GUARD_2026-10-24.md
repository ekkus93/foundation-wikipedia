# SRC-004 / SEC-001 — Stable Page-ID Alias Metadata Guard (2026-10-24)

## Scope

The online Action API current-revision request uses `pageids=<stable-id>` and never requests title normalization, redirects or interwiki resolution. A matching `pageid` is insufficient when a response includes unexpected `query.normalized`, `query.redirects` or `query.interwiki` metadata. Reject all three fields (including empty arrays) before creating an HTML oldid request.

## Regression coverage

- `crates/wiki-source/src/online_article.rs`: parser tests reject each unexpected marker, empty and populated, while accepting a normal stable page-ID revision response.
- `crates/wiki-source/src/online_fetch.rs`: fake transport confirms each unexpected marker halts at metadata, does not request rendered HTML, and retains a verified saved revision as an explicitly local fallback.
- Verify Rust workspace and platform CI against the exact source SHA after committing.

## Qualification limits

Synthetic response validation is not live Wikimedia API compatibility, verified transport TLS, publisher authentication, complete offline media, or desktop/Android device acceptance. SRC-004 and SEC-001 remain unchecked.
