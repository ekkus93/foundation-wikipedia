# RAG-003 — Reviewed-claim word boundaries and verdict coherence

**Date:** 2026-11-02 (America/Los_Angeles)  
**Exact passing source SHA:** `f97cd73f0ca9f38ded4eab2f14008af90559d2c1`  
**Status:** Partial; RAG-003 remains unchecked.

## Defect and implementation

The human-reviewed answer-support scorer previously checked whether the annotated claim text appeared as a literal substring in generated answer prose. Consequently, a reviewer could label `moon` as an answer claim when the answer only said `moons`, or `café` when the answer only said `cafés`. This could inflate supported-claim scores without the annotated claim actually being a whole word. A separate inconsistency allowed a claim labeled `unsupported` to contain a citation pair labeled `supported`.

The scorer now matches each claim at complete Unicode alphanumeric/underscore word boundaries. Existing citation-marker stripping still prevents words on either side of a marker from being concatenated. The evaluator also rejects an unsupported claim when any citation pair is explicitly marked supported, while retaining the requirement that supported claims have at least one supported citation. Unreviewed verdicts remain distinct from reviewed support.

## Regression and exact-head qualification

New unit regressions cover embedded ASCII and accented Unicode substrings, valid whole-word matches, contradictory unsupported-claim/supported-citation labels, and consistent supported labels. Existing strict canonical article membership and per-case gate tests remain in the Rust workspace suite.

The first source SHA `6e8e37187f9033ce4916844e8807b6e00129880c` failed Rust formatting ([run 41457343749](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41457343749)); the formatting-only correction `f97cd73f0ca9f38ded4eab2f14008af90559d2c1` passed all three exact-commit workflows:

- [Rust workspace 41457626315](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41457626315): format, strict Clippy and workspace tests passed.
- [Platform shells 41457626192](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41457626192): passed.
- [Android Rust ABI qualification 41457626257](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41457626257): passed.

## Qualification limits and next steps

This improves the integrity of caller-provided human annotations; it does **not** identify all unannotated factual claims, guarantee that claim spans do not overlap, prove citations are attached to the right occurrence, or judge whether a passage entails a claim. Unicode word-boundary checks are conservative for scripts without explicit whitespace boundaries; full multilingual segmentation needs separate qualification. Reviewer independence, Wikimedia publisher authentication, representative evaluation suites, model-to-evidence delivery, and real-device testing remain outstanding. RAG-003 stays unchecked.
