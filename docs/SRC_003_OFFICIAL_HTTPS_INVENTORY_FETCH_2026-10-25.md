# SRC-003: Official HTTPS inventory fetch (2026-10-25)

Status: partial qualification; SRC-003 remains unchecked.

## Operator command

Run `wiki-pack fetch-public-inventory PROJECT GENERATION EXPECTED_SHA256 PRIVATE_SOURCE_DIR`.
Create the destination directory with mode 0700 beforehand. Obtain the
expected SHA-256 digest independently of the fetched response.

The command constructs only the official Wikimedia current-content
SHA256SUMS URL and uses system curl with TLS peer verification, no redirect
following, connection/transfer timeouts, no user curlrc, an independently
enforced 32 MiB response cap and explicit HTTP 200 handling.

The exact bytes are checked against the caller's pinned digest and validated
as a correct generation-specific checksum inventory. The CLI refuses unsafe
or already occupied destinations, writes a private mode-0600 temporary file,
syncs it, publishes with an atomic no-overwrite hard link, and syncs the
directory on Unix. It never installs a pack or changes the active snapshot.

## Qualification

Implementation commit: `899ad2667b4c62497a627442b7d263f65c202ac9`.

Rust workspace CI passed:
https://github.com/ekkus93/foundation-wikipedia/actions/runs/40179007503

Platform shells CI passed:
https://github.com/ekkus93/foundation-wikipedia/actions/runs/40179007471

Regression tests use an offline fake curl executable to verify options and
response handling. Additional tests exercise invalid dates/projects/digests,
HTTP errors and redirects, oversized responses, no-overwrite publication,
symlink rejection and private-file permissions. There is no live upstream
download acceptance claim.

## Remaining

Neither certificate-validated HTTPS nor the caller-supplied SHA-256 pin proves
independent Wikimedia publisher provenance or release completeness. The
actual compressed shards, independent census authentication, revision-aware
renderer, mandatory media and desktop/Android end-to-end acceptance remain
unqualified. Do not mark SRC-003 complete.

Official layout documentation:
https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports
