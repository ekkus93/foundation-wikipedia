# SRC-004 / DUI-001 — Desktop title identity boundary alignment

**Date:** 2026-10-24
**Status:** Scoped code implementation; end-to-end acceptance remains open.

The desktop's untrusted IPC title validator and extracted article-link
navigation now mirror the Rust `safe_navigation_text` restrictions for
zero-width/invisible formatting characters and Unicode noncharacters,
including supplementary-plane noncharacters. Article links additionally
must meet the Rust exact-title request's 512-byte UTF-8 limit rather than
only a JavaScript UTF-16 character limit. Malformed UTF-16 in raw hrefs
is rejected before WHATWG URL parsing can replace lone surrogates and
silently change a navigation identity.

Regressions exercise IPC search/article titles, user title input and
extracted link destinations; valid Arabic/Persian/Indic joiners, Hebrew,
CJK, and emoji remain accepted. This is a local identity-consistency
and navigation-hardening change, not live Wikimedia interoperability,
offline visual completeness, installed pack validation or device acceptance.

**Verification:** CI evidence must be recorded against the exact committed
source SHA. Do not mark SRC-004 or DUI-001 complete on these tests alone.
