# STORE-002: Streaming offline media integrity audit (2026-10-26)

**Status:** Scoped media-verification improvement; STORE-002 and PACK-003 remain unchecked.

## Change

`MediaObjectStore::verify_object_size` verifies the existing content-addressed SHA-256 object in 8 KiB chunks and returns its verified byte length. `verify_declared_media` uses this streaming verifier instead of `read_verified`, which allocates a `Vec<u8>` up to the per-object 64 MiB cap. This bounds transient payload memory for pack-wide integrity sweeps independently of the number and size of required images. Actual rendering still uses the existing verified read API.

The streaming path retains strict lowercase SHA-256 identity, directory/symlink rejection, size cap, Unix inode identity and hardlink checks, pre/post-read size consistency, and full content hashing. It does not establish publisher authenticity or prove that every article visual was discovered.

## Tests and evidence

The `media_inventory` synthetic regressions verify a 4 MiB+13-byte object, exact expected byte count, rejection of same-length modified bytes, and Unix refusal of an external hard link. The existing manifest mismatch and corrupted-media tests continue to exercise failure paths.

The first implementation at `4e34f49cb8921d9f4f673fd61c382c0cd88053e2` was followed by a borrow-safe fixture correction at `43cb1b171637c4b5de215d1b6c39b58a1b52e640`; Rust CI [40361308072](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40361308072) failed rustfmt only. The corrected source SHA **`13f62a7fcc92e53c51ec62c144eb5128558c60cd`** passed [Rust workspace 40361472955](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40361472955), [Platform shells 40361472803](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40361472803), and [Android Rust ABI 40361472845](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40361472845). [Android record codec probe 40361472868](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40361472868) was still in progress when checked.

## Remaining qualification

Authenticated required-media manifests, full article-to-visual discovery, content-owner GC with concurrency-safe byte deletion, complete pack installation/activation, and physical desktop/Android airplane-mode acceptance remain required. No STORE-002/PACK-003 top-level completion is claimed.
