# RAG-003 semantic review manifest CI gate

The Rust workspace workflow now explicitly executes `python3 -m unittest discover -s scripts -p 'test_validate_rag003_review_manifest.py' -v` on every push to `master` and pull request. This makes the independent-review manifest's semantic regression suite a named, visible, failing CI gate, rather than relying solely on the broad fixture-provenance unittest discovery step. The broad step continues to run; this intentional duplication provides a clear qualification signal for RAG-003.

The gate checks synthetic fixtures only. It does **not** establish that a real manifest satisfies the JSON Schema, authenticate source checksums or reviewer identity, verify reviewer independence, or qualify factual entailment and physical-device acceptance. RAG-003 remains incomplete until real evidence is independently verified.
