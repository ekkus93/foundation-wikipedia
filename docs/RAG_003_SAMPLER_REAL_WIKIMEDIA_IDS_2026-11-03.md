# RAG-003 sampler: real Wikimedia source IDs (2026-11-03)

The original sampling utility applied its 100,000-candidate resource bound
to page and revision IDs. Real Wikimedia source IDs routinely exceed that
value, making legitimate inventories and exclusion records unusable.

The correction accepts positive signed-64-bit page/revision IDs while
retaining an independent 100,000-candidate bound on the minimum per-project
article floor and the inventory size. Regression tests cover large source
identities, exclusions, out-of-range identities, and excessive floors.

This is local input correctness, not verified source acquisition, genuine
human review, or RAG-003 qualification. RAG-003 remains unchecked.
