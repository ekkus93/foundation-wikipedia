# STORE-003 — Candidate-bound offline activation (partial)

**Implementation SHA:** `2f86a8b6c382a8f4f311f688c832549edd7ba3a3`

`ActiveSnapshotStore::switch_to_manifest_verified_with_bound_pack_media(id, store, registry, required, expected_shards)` first requires a valid staged `snapshots/<id>/manifest.json`. With one shared media lease, it verifies the bound media root, the exact pack owner `id`, all caller-declared mandatory media bytes, and the candidate's own `catalog.sqlite`, `search.sqlite` and manifest-listed shard lengths/SHA-256 using `VerifiedOfflineCatalogReader::open_with_manifest_shards`. Only after these checks succeed does the existing atomic selector publish `id`. The previous selector remains active on local validation failures.

The manifest inventory and mandatory media list are **caller-supplied** and must originate from independent, authenticated publication verification. This API does not authenticate Wikimedia, discover required media, prevent noncooperating filesystem replacement, implement a full pack installer, or qualify real-world power loss and devices. A partially supplied inventory must not be treated as a complete official pack.

A disk-backed regression demonstrates missing FTS index, forged expected shard digest, corrupted article shard and successful activation with a required installed image. Existing activation tests cover reader-callback failures, media corruption, invalid staging and selector rollback separately.

**CI on exact implementation SHA:** [Rust workspace (passed)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43171243788), [Platform shells (passed)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43171243810), [Android Rust ABI qualification (passed)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43171243796), [Android record codec probe (pending at documentation)](https://github.com/ekkus93/foundation-wikipedia/actions/runs/43171243811).

**STORE-003 remains unchecked.**
