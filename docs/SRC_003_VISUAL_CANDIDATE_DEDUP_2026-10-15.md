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
