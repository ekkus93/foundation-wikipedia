# ADR 0009 — Hosting official packs and their catalog

**Status:** Accepted (decision delegated by the owner to Claude on 2026-11-16, "Can't you just do this for me?"). **Nothing is published yet.** The first upload of a real pack will be confirmed with the owner, because it is public and permanent.
**Date:** 2026-11-16
**Requirements:** UPD-001, MOBPACK-001, TRUST-001 (ADR 0008)

## Context

MOBPACK-001 (Android) and the desktop app must discover official packs, download them resumably and verify them before activation. UPD-001 asks for a hosting, mirror and credential choice with stable URLs.

The project is a public GitHub repository, `ekkus93/foundation-wikipedia`. There is no budget or account for a separate CDN. Packs are signed with the publisher key pinned in the app (ADR 0008), so the host is **not** trusted for integrity: a modified or substituted file fails verification.

## Decision

**Host: GitHub Releases** of `ekkus93/foundation-wikipedia`.

**Release layout:**
- **Pack releases:** one release per published pack set, tagged `packs-<snapshot>` (for example `packs-20261101`). Its assets are the `<pack-id>.wpack` files and their `<pack-id>.wpack.sig` signatures.
- **Catalog:** the latest catalog is attached to a release tagged `catalog`. That release is re-pointed on each publication: delete the old assets, then upload the new ones.
- **Stable catalog URLs:**
  - `https://github.com/ekkus93/foundation-wikipedia/releases/download/catalog/catalog.json`
  - `https://github.com/ekkus93/foundation-wikipedia/releases/download/catalog/catalog.json.sig`

**Catalog (to be specified with MOBPACK-001).**
- **Contents:** a bounded, canonical JSON document listing each pack with:
  - pack ID, project, snapshot and title;
  - download URL and size;
  - the pack's manifest transcript SHA-256;
  - its signature file URL;
  - a monotonically increasing catalog sequence number, which defeats rollback to an older catalog.
- **Signing:**
  - It is signed with the publisher key under a **different domain prefix** from packs, `foundation-wikipedia/catalog-signature/v1\0`, so a pack signature can never be presented as a catalog signature, or the reverse.
  - A catalog lists packs. It never makes a pack trusted: every pack is still verified against its own signature and bytes before activation.

**Credentials.**
- **Uploading:** releases are created by the maintainer's own `gh` login on the workstation that holds the signing key.
- **CI:** no signing key and no release token is stored in CI.

**Downloads.**
- **Redirect:** GitHub answers a release-asset URL with a redirect to `objects.githubusercontent.com` (or `release-assets.githubusercontent.com`). The app's download client must follow **only** that one redirect, with HTTPS certificate verification and no other hosts. Today's transports refuse all redirects, so this is a deliberate, narrow exception, to be implemented and tested in MOBPACK-001.
- **Resuming:** HTTP Range requests, which those hosts support.
- **Disk check:** compare the catalog size with free space before starting.

**Limits and privacy.**
- **Size:** GitHub release assets must each be under 2 GiB. Larger packs need a multi-part format, which is a follow-up and not yet designed.
- **Privacy:** downloading reveals the user's IP address and the requested file to GitHub. The app must disclose this before the first download (spec §21) and honour the Wi-Fi/cellular setting.

## Alternatives considered

- **A dedicated object store or CDN** (S3, R2 and so on): needs an account, credentials and money. That's unnecessary while the host is untrusted for integrity and volumes are small. It can be added later as a mirror without changing trust, since packs are signed.
- **Wikimedia infrastructure:** not ours to publish to.
- **Git LFS in the repository:** bandwidth quotas, and every pack would bloat clones.
- **IPFS or BitTorrent:** good content addressing, but poor support on Android and in offline-first stacks, and it adds peer-to-peer disclosure.

## Status of evidence

This is a decision record only.
- **Not yet published:** no release, catalog or pack exists on GitHub.
- **Not yet built:** catalog signing and the narrow redirect are unimplemented.
- **Exists:** TRUST-001 signing and verification (ADR 0008).
