# RAG-003 — Reviewed claim text integrity

**Date:** 2026-11-02 (America/Los_Angeles)  
**Exact passing source SHA:** `57a6538ae23c9b4e9729ac97901e1ab4416019e2`  
**Status:** Partial. RAG-003 remains unchecked.

## Defect and scope

The human-reviewed answer-support scorer previously used `answer.contains(claim.text)`. That allowed a reviewer annotation to count text occurring **only inside a citation marker ID** as if it were generated answer prose. Because claims were deduplicated by ID alone, the same literal claim could be repeated under multiple IDs, inflating claim-support totals and, with repeated citation annotations, citation counts. Removing a marker without a separator would also join otherwise separate words into an invented span.

## Implementation

- Parse existing `[[cite:BLOCK_ID]]` markers as before, but separately construct the answer prose with marker text replaced by a space. Claims must be literal substrings of this visible prose; marker IDs cannot qualify as claim text.
- Reject duplicate literal claim text within the same answer even when claim IDs differ. Since schema-v1 claim annotations have no occurrence offsets, duplicate literal text cannot be reliably distinguished as separate occurrences. Different cases remain independent.
- Keep existing strict marker labeling, canonical project/page/revision identity, corpus-membership checks, score denominators and report schemas unchanged.
- Unit regressions verify marker-only claim rejection, duplicate text with different IDs, no splicing across markers and unchanged valid two-claim scoring.

## Verification

Source `57a6538ae23c9b4e9729ac97901e1ab4416019e2` passed exact-head GitHub Actions:
- [Rust workspace 41449830153](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41449830153): formatting, strict Clippy and Rust tests passed.
- [Platform shells 41449830089](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41449830089): passed.
- [Android Rust ABI 41449829985](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41449829985): passed.

Intermediate commits `d87e1828ce80cee11dea9bafdab298684d4f4720`, `f90bafbd9579ef7fee96096c8020363c3a7a6975`, and `0e4743a6e187a694b272972393f8c375969d6de9` failed the Rust format gate; the last intermediate failure was whitespace only. The passing source above restores the complete file and fixes formatting.

## Not qualified

This is annotation-integrity hardening, not independent factual entailment. Substring-based claim matching still does not prove exhaustive segmentation of generated prose or disambiguate repeated occurrences. Reviewer labels and canonical article inputs remain caller-supplied and are not independently authenticated as Wikimedia publisher output. Representative real-source, independently reviewed answer suites, full chat-flow evidence delivery and physical-device qualification remain outstanding. Do not mark RAG-003 complete.
