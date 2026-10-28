# STORE-002 — Manifest attribution preflight (2026-10-27)

## Scope and verified source

Source commit: `bf36a20c2ee608821967608bfca7b8a82937d8b2`. Scope: `crates/wiki-store/src/media_manifest_install.rs`.

The manifest-aware installer previously checked that notices were individually valid, but could publish content-addressed objects before its SQLite ownership transaction rejected two MIME declarations for one digest or a 1,025th distinct notice. Such failures did not activate a pack or register partial ownership; however, they created unnecessary unowned published bytes that required later orphan recovery.

The preflight now groups assets by SHA-256 digest and checks a single MIME per digest and a maximum of 1,024 distinct notice tuples (`mime`, `source_url`, `creator`, `license`) **before** acquiring the install lease or publishing bytes. Identical repeated notice tuples are counted once. The transactional registry checks remain authoritative and still run at commit time; these new checks are not a security substitute for SQLite atomicity.

## Automated qualification

A disk-backed root-bound regression `conflicting_mime_and_excessive_notices_fail_before_media_publication` verifies:

- Contradictory MIME for identical bytes fails with `InvalidManifest` and neither the content-addressed object nor pack owner claim exists.
- A batch with 1,025 distinct notices for one digest fails with `InvalidManifest` without file publication or owner claims.
- Repeated identical notices install successfully and result in exactly one persisted attribution notice.

Run the Rust suite with `cargo test -p wiki-store`; the exact-head Rust workspace CI includes format, Clippy, tests and repository-level gates.

Verified passing source workflows:

- [Rust workspace run 40612536891](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40612536891)
- [Platform shells run 40612536884](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40612536884)
- [Android Rust ABI qualification run 40612536947](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40612536947)

The first two source attempts failed only formatting: [run 40612386736](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40612386736) and [run 40612484785](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40612484785). The corrected source passed.

## Remaining acceptance limitations

This is local, synthetic attribution and failure-atomicity preflight only. The caller still must authenticate upstream media and source manifests, prove the completeness of every required visual for exact article revisions, safely integrate installed pack/catalog/snapshot activation and removal, qualify power-loss/disk-full and hostile-writer scenarios, and pass real desktop/Android offline acceptance. The Android ABI gate is not a real-device installation test. **STORE-002 is not complete.**
