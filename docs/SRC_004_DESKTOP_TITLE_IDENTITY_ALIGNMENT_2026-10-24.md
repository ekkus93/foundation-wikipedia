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

**Exact-head verification:** Implementation SHA `f9845bda4985b3f96df532d1dd71cd3e68cc5ab9` passed [Rust workspace 40143891487](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40143891487), [Platform shells 40143891381](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40143891381) (frontend typecheck, Node tests and build), and [Desktop launch smoke 40143891364](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40143891364). These tests do not complete SRC-004 or DUI-001.
