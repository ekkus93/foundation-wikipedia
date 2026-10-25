# SRC-004 / DUI-001 — Desktop article-link display-label Unicode isolation

**Date:** 2026-10-24
**Status:** Scoped implementation and exact-head CI passed; acceptance remains open.

Article links extracted from untrusted Wikimedia HTML previously sanitized only
C0/C1 controls and a subset of bidirectional marks in visible link labels.
Their **destinations** were already validated, but labels could still carry
invisible U+200B/U+2060/U+FEFF, Unicode noncharacters (including plane-ending
noncharacters), malformed UTF-16, and supplementary-plane truncation artifacts.
Such labels could visually misrepresent a legitimate destination.

The desktop extraction now shares the Rust-aligned unsafe-navigation Unicode
predicate with IPC title validation. It removes invisible formatting and
noncharacters from link **labels**, replaces C0/C1 and Unicode line/paragraph
separators with spaces, drops unpaired surrogate code units, and truncates at
160 Unicode scalar values rather than splitting a surrogate pair. Empty labels
fall back to the validated destination title. Legitimate Persian ZWNJ and
Indic ZWJ remain intact. Destination validation and the detached, sandboxed
HTML template boundary remain unchanged.

**Regression:** `desktop/scripts/articleLinks.test.mjs` exercises invisible
controls, noncharacters, malformed UTF-16, bidi controls, C1 whitespace,
multilingual joiners, fallback labels and 160-emoji truncation. The initial
test fixture mistakenly used literal double-escaped Unicode and failed
[Desktop smoke 40147328342](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147328342);
the corrected fixture tests actual Unicode scalars.

**Implementation:** `a8e6b392748131583a0faeb80fb6d228f815bbf2`;
corrected regression at `d41e22fe2ab61f865f84fbbb7aac46212ddbf40e`.
Exact-head CI on the corrected code:
- [Rust workspace — success (40147438846)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147438846)
- [Platform shells — success (40147438754)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147438754)
- [Desktop launch smoke — success (40147438973)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40147438973)

This is a narrow display-security improvement. It does **not** establish live
Wikimedia interoperability, complete revision-matched offline content/media,
installed pack integration, Android parity or physical-device acceptance.
**SRC-004 and DUI-001 remain unchecked.**
