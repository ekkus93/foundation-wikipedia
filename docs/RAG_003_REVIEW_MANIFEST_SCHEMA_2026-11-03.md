# RAG-003 review evidence manifest schema

`docs/schemas/rag003-review-manifest.schema.json` defines a Draft 2020-12 JSON Schema for recording preregistration, revision-pinned Wikimedia sources, raw reviewer judgments and reproducible runner execution. It is an **evidence structure**, not evidence that any review occurred.

Validate with any Draft 2020-12 validator that enables `format` assertion (date-time and URI). JSON Schema alone cannot prove publisher authenticity, enforce two *distinct* reviewers per case, compare byte-offset boundaries to the original answer, ensure unique project/page/revision keys, ensure a review source exists in `source_articles`, prove the sampling plan predates execution, or establish independent human review. These must be verified in a subsequent semantic validator and external audit. Preserve raw individual reviews; do not overwrite disagreements with adjudicated consensus. Store real evidence only after collection, with appropriate privacy review.

Status: **partial RAG-003 infrastructure only; RAG-003 remains unchecked**. No fabricated source, reviewer, benchmark, or device evidence is included.
