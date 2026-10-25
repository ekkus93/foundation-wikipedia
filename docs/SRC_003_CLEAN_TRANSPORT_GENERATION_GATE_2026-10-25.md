# SRC-003 — Read-only clean staged-generation verification (2026-10-25)

Status: scoped acquisition-integrity increment; **SRC-003 remains unchecked**.

A new host CLI command, `wiki-pack verify-public-members <project> <generation>
<SHA256SUMS-sha256> <private-source-dir> <max-compressed-bytes-per-member>`,
performs a read-only integrity gate after `fetch-public-members-resume`.

It rereads the private `SHA256SUMS` with the independently supplied digest,
checks every declared member's full SHA-256 and compressed byte budget, checks
the aggregate generation bound, and rejects missing, corrupted, symlinked or
hardlinked files. It also enumerates the entire private acquisition tree and
rejects any unlisted file, directory, symlink or leftover partial checkpoint.
Nothing is downloaded, deleted, published, installed or activated by the gate.

Synthetic tests cover a missing second shard, successful two-shard verification,
wrong project, too-small budget, leftover checkpoint, digest corruption,
hardlinked member and unlisted symlink. The verifier intentionally handles
the **transport-only** staging layout (SHA256SUMS plus declared members);
publisher `PUBLICATION.json` manifests require a separate authenticated
source-acceptance workflow and are not implicitly trusted here.

**Not established:** independent Wikimedia publisher authentication, complete
source generation/census, revision-matched HTML, mandatory offline media,
desktop/Android acceptance or installed pack integrity.
