# RAG-003 semantic review manifest validation (2026-11-03)

Run `python3 scripts/validate_rag003_review_manifest.py manifest.json` and `python3 -m unittest discover -s scripts -p 'test_validate_rag003_review_manifest.py'`.

The standard-library semantic validator rejects duplicate source revisions, duplicate reviewer judgments, references to absent source revisions, invalid claim byte ranges, review/acquisition dates predating preregistration, missing required project coverage, and malformed execution checksums. It rejects duplicate JSON object keys.

**Important:** Run JSON Schema Draft 2020-12 validation separately against `docs/schemas/rag003-review-manifest.schema.json`. This tool does not verify actual SHA-256 content, publisher signatures, reviewer independence, source authenticity, annotation factual correctness, or physical-device acceptance. Synthetic fixtures are not qualification evidence. RAG-003 remains unchecked.
