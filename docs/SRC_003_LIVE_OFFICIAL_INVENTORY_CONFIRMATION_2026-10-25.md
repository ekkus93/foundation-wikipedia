# SRC-003: Live official HTTPS inventory confirmation (2026-10-25)

Status: partial official-origin qualification; **SRC-003 remains unchecked**.

The host-only `wiki-pack confirm-public-inventory [--json]` command rereads
the locally staged, independently SHA-256-pinned `SHA256SUMS` under the
private-root and anti-alias checks, then **always performs a fresh**
certificate-verified HTTPS request to the canonical
`dumps.wikimedia.org/other/mediawiki_content_current/{project}/{generation}/xml/bzip2/SHA256SUMS`
location. The response must match the exact project, generation and caller
pin. It is compared with the staged inventory, and mismatches, redirects,
HTTP errors and TLS failures are fatal. The command is read-only: no shards
are downloaded, no staging files are modified, and no installed pack changes.

`--json` emits a single schema-v1 live confirmation receipt **only** after
success. It distinguishes a fresh official HTTPS inventory observation from
an authenticated publisher **signature**, an independently validated page
census, revision-matched rendering, mandatory offline media and pack
installation; those stronger claims remain false. A receipt is an ephemeral
observation, not a durable signed provenance artifact. The command does
not alter the offline replay behavior of `fetch-public-generation`.

Synthetic regressions cover successful pinned live recheck, mismatch,
transport failure, malformed/mismatched identity, tampered local inventory,
hardlink alias rejection, and absence of staging writes.

Wikimedia's [MediaWiki Content File Exports documentation](https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports)
describes published `SHA256SUMS` availability as the completion signal for
the new current-content export, and advises verifying every listed file.
Its availability does **not** independently prove page-level completeness.
The separate [legacy dumps sunset notice](https://wikitech.wikimedia.org/wiki/Legacy_content_dumps_sunset)
states that the new export is **not** tracked by legacy `dumpstatus.json`;
this implementation deliberately does not use that unrelated endpoint.
