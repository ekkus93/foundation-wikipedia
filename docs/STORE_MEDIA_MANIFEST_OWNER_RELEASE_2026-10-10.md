# Manifest-bound media installation and atomic owner release — partial STORE-002 / PACK-003

Date: 2026-10-10 (America/Los_Angeles)

## Implemented

- `wiki-store::media_manifest_install::install_manifest_media_batch` checks an independently authenticated required-media inventory against the exact SHA-256 digest and byte length of each supplied media asset *before* any object is staged or registered. It rejects missing, extra, duplicate required digests, malformed hashes, mismatched sizes and invalid attribution. Repeated identical media bytes may carry distinct creator/license notices.
- Existing `install_owned_media_batch` then stages all content-addressed bytes before registering the complete owner/notice set in one SQLite transaction. Regression tests exercise disk-backed reopen, shared content, multiple notices, missing/extra inventory and rejection without writes.
- `MediaRegistry::remove_all_for_owner` releases all ownership claims for one pack/cache/pin owner in a single SQLite transaction. The returned digests are informational, **not** permission to delete bytes; independent pack ownership and user pins remain intact. A shared-owner regression test exercises the release path.

## Qualification

- Manifest-bound installer committed at `0cd0b4668d521113e82e5ac26e8628d110daae38`, rustfmt-corrected at `b3af1bcceaff780222d4653409ed968a03f3b934`. Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38102391787 (passed).
- Atomic owner-release implementation committed at `1a2cab1b2a4e81dd165031e2c3d7736209cf8f30`, with formatting fixes through `6f2fe27dd129e11d841dedfd1bc5ff75cb07b113`.
- Exact code head `6f2fe27dd129e11d841dedfd1bc5ff75cb07b113`: Rust workspace CI https://github.com/ekkus93/foundation-wikipedia/actions/runs/38102758605 (passed), Platform shells CI https://github.com/ekkus93/foundation-wikipedia/actions/runs/38102758620 (passed), Android Rust ABI qualification https://github.com/ekkus93/foundation-wikipedia/actions/runs/38102758610 (passed). Android record codec probe https://github.com/ekkus93/foundation-wikipedia/actions/runs/38102758559 was still running when this evidence was prepared; do not assume it passed.

## Remaining acceptance gates

**STORE-002 and PACK-003 remain unchecked.** This implementation does not authenticate the caller's manifest or prove every rendered article visual was discovered; it is not a portable .wpack installer, an atomic pack/snapshot activation transaction, or a crash-safe physical media garbage collector. Concurrency-safe byte deletion, authenticated full-manifest coverage, renderer/CSS/font/math resource closure, attribution presentation, and real Android airplane-mode visual acceptance are still required. Neither signed-publisher status nor offline-ready status may be inferred from these helper APIs.
