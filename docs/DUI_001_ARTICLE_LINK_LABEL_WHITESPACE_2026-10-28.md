# DUI-001 — Extracted article-link label whitespace normalization

Date: 2026-10-28. Scope: desktop online text-preview article-link list; not an offline-ready reader.

## Defect and fix

The extracted-link display sanitizer already removed Unicode identity-spoofing controls, mapped C0/C1 controls to spaces, preserved legitimate ZWNJ/ZWJ, and bounded labels by Unicode scalar count. Its final whitespace-collapse expression mistakenly matched a literal backslash followed by `s`, rather than actual whitespace. Consequently a label containing multiple spaces, tabs, line breaks, nonbreaking spaces, or Unicode paragraph separators could remain visually fragmented even though individual control characters were mapped to spaces.

`desktop/src/articleLinks.ts` now collapses actual Unicode whitespace with `/\s+/gu` (the regex source has **one** backslash). It retains the existing fail-closed link destination validation, duplicate-title suppression, scalar-aware truncation, and sanitized fallback title. No iframe privileges, remote asset permissions, or network paths were changed.

`desktop/scripts/articleLinks.test.mjs` now covers repeated spaces, tab/newline, U+00A0, U+2028/U+2029, and literal backslash-s text. Existing RTL joiner, emoji, spoofing-control and fallback-title cases remain.

## Exact-head verification

Implementation commit: `a84767952e3c55b462f09b9340f5098a9155de48`.

- [Rust workspace CI 40681654586](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40681654586) — success.
- [Platform shells CI 40681654590](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40681654590) — success.
- [Desktop launch smoke CI 40681654546](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40681654546) — success, including frontend tests.

## Qualification boundary

This is a bounded display-text regression fix only. Publisher-authenticated complete source ingestion, installed offline catalog/media navigation, iframe reading-position restoration, physical desktop/Android acceptance, and high-fidelity math/image rendering remain unqualified. **DUI-001 and SRC-004 remain unchecked.**
