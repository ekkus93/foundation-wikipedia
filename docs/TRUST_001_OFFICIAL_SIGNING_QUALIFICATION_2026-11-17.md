# TRUST-001 — official signing and trust-root verification (2026-11-17)

TRUST-001 was waiting only on its dependency PACK-001. The owner ruled PACK-001 done on 2026-11-17. This note maps the existing evidence to TRUST-001's subtasks and acceptance criterion. No signing code changed for this qualification.

## Subtask 1: signature scope, pinned key, rotation/revocation, test vectors

- **Signature scope:** `wiki_pack_format::signature` signs the canonical manifest identity transcript from ADR 0007, under a domain-separated prefix. That transcript commits to every object's SHA-256, so the signature covers every byte of the pack. See [ADR 0008](adr/0008-official-pack-signing.md).
- **Pinned publisher key:** `OFFICIAL_KEYS` pins publisher `foundation-wikipedia`, key ID `ee24d083334a607f`, valid from snapshot `20260101`.
  - Test: `the_production_keyring_pins_the_publisher_key_by_its_id`.
  - The secret key is kept outside the repository.
- **Rotation and revocation procedure:** ADR 0008 (Accepted) covers planned rotation (`valid_until`), revocation after a compromise (`revoked: true` plus an app update), and custody.
  - The key must move offline, or be rotated, before the first public release.
- **Test vectors:** committed in `fixtures/synthetic/signature/` (`committed_signature_test_vectors`). OpenSSL 3.5 verified them independently, and rejected a one-byte change.

## Subtask 2: "Verified publisher" requires origin plus integrity, not just a checksum

A pack is labelled Verified publisher only when all of the following hold:

- every object byte matches its manifest digest;
- the signature verifies over the staged manifest transcript;
- the signing key is pinned, unrevoked, belongs to the manifest's publisher, and its validity window covers the snapshot.

Tests: `only_pinned_unrevoked_keys_of_the_same_publisher_within_validity_are_trusted` and `a_signed_official_pack_installs_as_verified_publisher_until_its_key_is_revoked`.

Listing removes the label once a key is revoked or unpinned.

## Acceptance: a tampered or forged official pack cannot install or display as verified

| Case | Test |
|---|---|
| Tampered manifest or signature | `tampering_with_the_manifest_or_signature_is_refused` |
| Tampered payload, forged signature, unpinned key, signature for another pack: never installs, staging left empty | `wiki-store` `a_tampered_or_wrongly_signed_official_pack_never_installs` |
| Unsigned official claim refused before confirmation | `unsigned_official_claim_is_refused_before_confirmation` |
| Android catalog path: substituted or tampered download | `wiki-ffi` `a_substituted_or_tampered_pack_never_installs`, `an_older_or_tampered_catalog_is_refused_and_the_stored_one_kept` |
| Desktop: an official claim without a verifiable signature | refused with "…not installed" and never shown as verified (`offline_packs.rs` message test) |
| Custom pack on the official path, or masquerading as official | refused; TRUST-002 surfaces |
| CLI: an official claim read from stdin | `verify_stdin_cli` reports "Publisher signature: NOT VERIFIED" |

End to end with the production key (2026-11-16):

1. `export-official`, then `sign-pack`, then `verify-pack-signature` against the compiled keyring reported Verified publisher.
2. The same signature on a pack with one byte changed was refused.

The Android emulator installed the signed test release from its public URL (`OfficialPackLiveTest`, MOBPACK-001).

## Local verification at this head

`cargo test --workspace --locked` passes, including every test named above.

## Found while qualifying

`tools/wiki-pack/tests/verify_stdin_cli.rs` failed once locally with a broken pipe:

- **Cause:** with a `0` budget, the CLI correctly refuses before reading stdin, and the test's write raced that exit.
- **Fix:** the harness now accepts `BrokenPipe` and checks exit status and stderr as before. Thirty consecutive reruns passed.

## Still outside TRUST-001

- Moving the signing key offline before the first public release (ADR 0008).
- Signed revocations in the catalog (UPD-001).
