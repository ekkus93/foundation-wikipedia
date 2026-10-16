# SRC-003 HTML base-URL resolution for visual candidates (2026-10-16)

Diagnostic only. This does not download, authenticate or certify any media.
`required_media_verified` and `css_and_font_dependencies_verified` stay `false`;
`SRC-003` and `PACK-003` remain open.

## Change

`scripts/inventory_rendered_visuals.py` `inventory()` accepts an optional
`document_url` (absolute http(s), same URL checks as candidates).

- Without it: behavior unchanged; raw candidates, `html_base_resolution_pending`
  is true when a `<base href>` exists, digest domain `v1`.
- With it: every candidate (HTML attributes, srcset, SVG, inline `<style>` and
  `style=""` CSS) resolves via `urljoin` against the HTML base, else the
  document URL. Scheme-relative URLs take the base scheme. Digest domain `v2`
  binds the document URL. Output adds `document_url` and `html_base_url`.
- Fail-closed: `<base href>` after any resource reference (both modes), empty,
  fragment-only, padded, credentialed, or non-http(s) base (data, javascript,
  blob, ftp), duplicate `href` attribute on `<base>`, a second `<base href>`,
  resolved URLs over `MAX_URL_CHARS`, invalid `document_url`.
- `<base>` inside `<svg>` or `<math>` is foreign content and is ignored.
- `inventory_css_dependency_graph` is unchanged: external CSS resolves against
  its own stylesheet URL, never the HTML base (regression-tested).

## Non-claims

Only the first-`href` rule is simplified to rejection of a second base. No
fully compliant HTML tokenizer; no network, redirect or byte verification.

## Qualification

Commit `c836b92a029b2b2189cf97304f37b4c898b396c8`, exact-head CI:
Rust workspace `38853517679`, Platform shells `38853517763`, Renderer dump
qualification `38853517725` all succeeded. Local: 381 Python tests pass.

Process note: the repo commit hook rejects `Co-Authored-By` trailers, so the
commit has none. No Bridge or CI defects were observed.
