# SRC-003 — cross-page visual URL deduplication and candidate fingerprint (2026-10-15)

The diagnostic candidate list now has a domain-separated, deterministic
`inventory_sha256` over a canonical sorted JSON array of (kind, URL)
entries. Duplicate/order-only rendered references retain the same digest,
while a different URL changes it. This digest is **not** source authenticity,
a verified media-object hash, a license receipt, or proof of exact template
dependency revisions.

The 25-page renderer aggregate now distinguishes total candidate references
(counted once for each page/kind) from distinct (kind, URL) candidates across
pages. Deduplication is observational; independently referenced thumbnail
sizes and URL variants are separate candidates until a later media downloader
verifies actual bytes/content identity. All results are bounded by the
per-page inventory caps, remain in retained CI artifacts, and cannot produce
an offline-ready pack without complete CSS/font discovery, Commons metadata,
authenticated downloads and installed-object verification.

**Qualification:** exact-head Rust/Python, Platform and renderer CI must pass.
Canonical SRC-003/PACK-003 checkboxes remain unchecked.

## Qualified source and CI coverage

The candidate-fingerprint/deduplication implementation landed directly on
`master` at `c28a562b9c5f8b3dea32c358b1feadb38be36a2f` via Ralph
operation `75534212a9794614808a7e7b2a5ff3da`. A narrow follow-up at
`cfad4f7e1f932b80b82e502b06eead04578dbf61`, operation
`14faba1893044f25a6049c56daa212bd`, clarified the NUL-delimited
fingerprint transcript and added a deterministic expected-digest fixture.
No initial source/test CI failure was required for that correction.

The dump renderer workflow originally omitted the newly added visual
inventory Python module and its tests from the push path filters/unit test
command. At exact head `3ad2bf0c3562e24ef0167ccebd118eead5ba1e95`,
operation `9d04b8a6e9e14d0e864e6db83340226b`, those dependencies were
added to `.github/workflows/renderer-dump-qualification.yml`.

All final exact-head workflows **passed**:
[Rust 38826792734](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38826792734),
[Platform 38826792795](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38826792795),
and [Renderer dump 38826792779](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38826792779).
Renderer job `116736008864` shows the 25-page diagnostic emitted **533
per-page live URL-kind candidates versus 25 local candidates**, but only
**441 unique live candidates versus 3 unique local candidates** when
deduplicated by URL and resource kind across articles. The 316-vs-zero
image-reference discrepancy and 23 pages with inline CSS persist.
URL-kind uniqueness cannot substitute for byte-level deduplication or
license/creator validation, and none of these metrics qualify offline-ready
articles or packs.

## Ambiguous data-URL responsive image protection

Diagnostic `srcset` splitting deliberately is not a full WHATWG grammar
implementation. A `data:` candidate contains a comma separating metadata
from its payload, which must not be misinterpreted as a second relative
remote-media filename. The candidate inventory now fails closed when it sees
a `data:` candidate in `img` or `picture/source` responsive variants,
with a deterministic regression. This protects the candidate list from a
false external-resource identity. It does not make the remaining simplified
`srcset` grammar suitable for certifying mandatory offline media.

## Exact-head qualification of ambiguous data-URI protection

The fail-closed data-URI-in-srcset regression is committed directly to master
at `ba93bd1f03fe155ca69742852a6ed866c21abc64` by Ralph Bridge operation
`dff2ca61449a4cdeb53f7475aa746f33`, `accepted=true`,
`write_reconciled=false`. All exact-head workflows passed:
[Rust 38827278461](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38827278461),
[Platform 38827278501](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38827278501),
and [Renderer 38827278454](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38827278454).
The update rejects malformed/unsupported responsive-media candidates rather
than falsely treating them as verified local assets. It does not establish
complete srcset or CSS parsing, Commons media provenance, verified offline
resource bytes, or any release acceptance.
