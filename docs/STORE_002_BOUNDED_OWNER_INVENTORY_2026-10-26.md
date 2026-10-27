# STORE-002 — Bounded owner inventory and persisted size claims (2026-10-26)

**Status:** Scoped read-side hardening only; STORE-002 remains unchecked.

`MediaRegistry::owned_media` now bounds pack/cache/user-pin inventory enumeration at **100,000 objects**, matching the maximum accepted by `register_verified_owned_batch`. The SQLite query requests at most 100,001 rows and rejects an over-limit result as `TooManyCandidates`; it never treats a truncated inventory as complete. The helper accepts smaller bounds for deterministic regression tests. Pack-media verification and manifest comparison already consume this API and therefore fail closed on excessive owner fanout.

The owner inventory and `owned_digest_size` now reject corrupt positive SQLite size claims above the content-addressed store's **64 MiB per-object limit** (and zero/negative values). An invalid claim cannot silently reach a reader or satisfy a manifest comparison. Owner-scoped lookups for a different pack still return no media. This does not rehash bytes or authenticate notices by itself.

**Tests:** Synthetic SQLite regressions verify three owned digests, exact-bound success, two-entry overflow rejection, zero-limit rejection, impossible positive declared size rejection in both APIs, and cross-owner isolation. Existing media-install/manifest and integrity tests remain covered by the Rust workspace suite.

**CI qualification:** Initial code SHA `f8decae37fd61ff9d41dd752909529875feb80af` failed rustfmt ([Rust workspace 40427334603](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40427334603)); corrected code SHA **`9f4926f11e33f6e947c7f3e8412c850f3b746b6d`** passed [Rust workspace 40427397063](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40427397063), [Platform shells 40427397052](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40427397052) and [Android Rust ABI 40427397126](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40427397126). Android record codec probe 40427397154 remained in progress when recorded.

**Limitations:** This does not provide a concurrency-safe garbage collector. SQLite and filesystem reads are not atomic across installers/readers, nor does the registry prove that the official publisher authenticated the media or that every revision-required visual is available. Safe deletion, crash recovery, complete offline visual coverage, pack activation, and physical-device acceptance remain outstanding.
