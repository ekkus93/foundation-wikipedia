# RAG-003 — Word boundaries bound to exact reviewed claim spans

**Date:** 2026-11-02 (America/Los_Angeles)  
**Passing implementation SHA:** `fe090a76831d1ba121a51729b5ac4a66e6adcea7`  
**Status:** Partial; RAG-003 remains unchecked.

## Defect

Schema-v2 annotations provide exact byte offsets, but the scorer previously validated word boundaries by searching *any* occurrence of the same literal text in the marker-stripped answer. A reviewer could label the substring `moon` inside `moons` and pass the boundary check if an unrelated standalone `moon` appeared elsewhere. Citation locality and UTF-8 span equality did not close that loophole.

## Change

The scorer now checks word boundaries against the **specific original-answer byte span** provided by the schema-v2 reviewer, after verifying UTF-8 character boundaries and excluding citation-marker intersections. The shared helper preserves the existing Unicode alphanumeric/underscore boundary semantics. Schema-v1 keeps its historical text-search behavior; schema-v2 no longer uses an unrelated visible-text occurrence to validate a particular annotation.

## Verification

The regression constructs a generated answer with both `moons` and a separate `moon` citation. It proves that a span inside `moons` is rejected, while moving the span to the exact standalone word passes. Additional negative cases cover an embedded prefix (`premoon`) and a Unicode suffix (`cafés` versus `café`), each with a separate valid occurrence elsewhere.

Initial source `9cf4b53adb0961e9d51845399e0e3e48f1be436a` failed only rustfmt ([Rust run 41476573372](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41476573372)); the formatting repair passed all observed workflows at the exact source SHA:

- [Rust workspace 41477038031](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41477038031): formatting, Clippy, workspace tests — **success**.
- [Platform shells 41477038029](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41477038029): **success**.
- [Android Rust ABI qualification 41477038042](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41477038042): **success**.

## Limits and next work

This is a structural claim-annotation integrity fix, not a factual-entailment engine. It does not prove that every generated claim was annotated, that a cited passage supports the claim, that reviewers are independent, or that the corpus has authenticated Wikimedia provenance. Continue RAG-003 with representative independently reviewed article/citation suites, verified source provenance, factual entailment and real-device qualification. Do not mark the task complete.
