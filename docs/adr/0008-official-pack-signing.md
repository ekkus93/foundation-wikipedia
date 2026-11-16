# ADR 0008 — Official `.wpack` publisher signatures

**Status:** Accepted in part. The owner chose the scheme on 2026-11-16: Ed25519 with `ed25519-dalek`, in our own small format, with publisher keys pinned in the app. That scheme is implemented in `wiki-pack-format::signature` and `wiki-store::pack_install::install_official_pack*`. The key-management procedure below is **proposed**. No production publisher key exists yet, so nothing can show "Verified publisher".
**Date:** 2026-11-16
**Requirements:** TRUST-001, TRUST-002, MOBPACK-001, UPD-001

## Context

ADR 0007 keeps the `.wpack` envelope unsigned: origin is only a claim, and the installer refused every official-origin pack. The spec (§8, §21) requires "Verified publisher" to mean origin under a pinned-key policy. It must never mean "the checksums match". The spec also requires that an unrecognized key never confer trust.

## Decision

**What is signed.** The pack's canonical manifest identity transcript (ADR 0007). The transcript commits to every object's path, SHA-256 and length. The installer verifies every object byte against the same manifest while staging, so a valid signature authenticates every installed byte.

The signed message is `SIGNED_PREFIX || SHA-256(transcript)`, where `SIGNED_PREFIX` is `foundation-wikipedia/pack-signature/v1\0`. The prefix is domain separation, so the message can never be a valid signed message in another context.

**Where it travels.** A separate signature file next to the pack: in the catalog for downloads, or `<pack>.wpack.sig` beside a file. The `FWPACK01` framing is unchanged.

**Signature file (v1).** UTF-8 with LF line endings, exactly four lines, at most 512 bytes. A parser accepts only the byte-identical canonical form:

```text
foundation-wikipedia/pack-signature/v1
key-id <16 lowercase hex: first 8 bytes of SHA-256 of the public key>
transcript-sha256 <64 lowercase hex>
signature <128 lowercase hex Ed25519 signature>
```

**Verification** (`signature::verify`) requires all of the following, and reports each failure distinctly:
1. The pack claims official origin.
2. The signature's transcript digest equals the manifest's.
3. A key with that ID is in the app's pinned keyring.
4. That key is not revoked.
5. The key's publisher equals the manifest's `publisher`.
6. The manifest's `snapshot` is a `YYYYMMDD` date inside the key's validity window.
7. Ed25519 `verify_strict` passes.

**Installer and labels.**
- **Installation:**
  - `install_official_pack` and `install_official_pack_stream` install an official pack only after this verification passes. The check runs on the staged manifest, after every object byte has been verified.
  - An official pack without a signature is still refused (`UnsignedOfficialPack`).
  - A failed verification is refused with its reason, and nothing is published.
  - A custom pack cannot use the official path.
- **Recorded key:** the installed `manifest.json` records the publisher and key ID.
- **Labels when listing:** listing keeps "Verified publisher" only while that key is still pinned and unrevoked. Otherwise it shows "Verification failed".

**Pinned keys.** `signature::OFFICIAL_KEYS` is the production keyring, compiled into the app. It is **empty** until the publisher key is generated. Tests use throwaway keys only.

## Key management (proposed procedure)

- **Generation and custody:**
  - The owner generates the Ed25519 key offline.
  - The 32-byte secret is kept off any CI runner and out of this repository: an offline machine or hardware token, with an encrypted offline backup.
  - Only the public key is committed, as a `TrustedKey` entry with publisher, `valid_from`, `valid_until: None` and `revoked: false`.
- **Signing:** done by the owner on the offline machine, over the exact `.wpack` that will be published. Before upload, the result is checked with the same verifier against the pinned keyring.
- **Tooling** (`wiki-pack`, host only):
  - `signing-key-generate <new-key-file>`: draws 32 bytes from OS randomness into a new file with owner-only (0600) permissions. It never overwrites an existing file and prints only the public key and key ID.
  - `sign-pack <key-file> <official.wpack>`: refuses a key file readable by others and verifies every pack byte first. It then writes `<pack>.sig`, refusing to overwrite an existing one.
  - `verify-pack-signature <pack> <sig> [<publisher> <public-key-hex> <valid-from>]`: checks against one given key, or against the app's pinned keyring.
- **Rotation (planned):**
  1. Add the new key with a `valid_from` date.
  2. Set the old key's `valid_until` to the last snapshot date it signs.
  3. Ship that app update before publishing packs signed by the new key.

  Packs already installed keep verifying under the old key's window.
- **Revocation (compromise):**
  1. Set `revoked: true` on the key and ship an app update.
  2. Installed packs signed by it then list as "Verification failed".
  3. Re-sign the affected packs with a new key, or withdraw them.

  A signed catalog (UPD-001) may later carry revocations between app updates. Its own signing key must then follow this procedure.
- **Never:** pin a key the app has not shipped, trust a key because a pack names it, or treat a matching checksum as publisher proof.

## Alternatives considered

- **minisign/signify file format.** Also Ed25519, and interoperable with the `minisign` CLI. It needs more parsing for comments and trusted comments, and has no field for our transcript digest. Declined by the owner in favour of the smaller own format.
- **Sigstore/cosign.** Keyless signing depends on online transparency-log and certificate checks, which an offline Android reader cannot perform.
- **Embedding the signature in the envelope.** That would change the accepted `FWPACK01` framing (ADR 0007), and the signature would have to exclude itself from the bytes it covers. A detached file avoids both.

## Evidence

- `wiki-pack-format` unit tests:
  - round trip and determinism;
  - tampered manifest or signature, and a signature for another pack;
  - unknown, revoked and wrong-publisher keys, a rotated-out key, an undated snapshot, a custom pack, and the empty production keyring;
  - exact-form parsing.
- **Committed test vectors** in `fixtures/synthetic/signature/`: transcript, signature and public key from the throwaway key `[7; 32]`. OpenSSL 3.5 verified them independently on 2026-11-16 (`openssl pkeyutl -verify -rawin` over `SIGNED_PREFIX || SHA-256(transcript)`) and rejected a one-byte change to the message.
- `wiki-pack` CLI test `signing_cli`, using a freshly generated throwaway key:
  - the secret is never printed, the key file is 0600, and an existing key is not overwritten;
  - a signed pack verifies with the given key but not with the empty pinned keyring or another publisher;
  - a tampered pack is neither signed nor verified, a custom pack is not signed, and a key file readable by others is refused.
- `wiki-store` installer tests:
  - a signed official pack installs as "Verified publisher" and reads back;
  - its label becomes "Verification failed" when the key is revoked or unpinned, and under the empty production keyring;
  - a tampered payload, a forged signature, an unpinned key and a signature for another pack never install and leave staging empty;
  - a custom pack is refused on the official path.
