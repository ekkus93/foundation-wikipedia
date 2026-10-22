# Offline Wikidata exact-revision qualification — 2026-10-22

Code `a274f792acf857220d65f6a737a9a30d25400f2e` adds `OfflineReader::open_exact_wikidata_revision` and a synthetic regression for valid, stale, invalid, missing and tampered offline article identities.

Rust workspace CI 39872590246, Platform shells CI 39872590167 and Android ABI CI 39872590189 passed. Android codec probe CI 39872590274 was still running when checked.

STORE-001 and SRC-004 remain unchecked: upstream authentication, mandatory offline media, installed pack integration and physical-device acceptance remain open.
