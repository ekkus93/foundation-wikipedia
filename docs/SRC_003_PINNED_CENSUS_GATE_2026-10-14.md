# SRC-003 — Digest-pinned independent census comparison (partial)

Date: 2026-10-14  
Status: Qualified code and synthetic integration; SRC-003 remains unchecked.

## Implementation

`wiki-source::public_dump_census_gate::audit_pinned_census_files` compares a caller-supplied, externally pinned expected page-ID census with the observed census from pinned public-dump ingestion. It hashes the expected file with SHA-256 before and after the streaming comparison. Both inputs must be nonempty regular files (not symlinks), under the 2 GB input ceiling, and cannot alias the same file/inode. Strict LF-terminated, increasing, positive decimal IDs and exact project/release/namespace scope are enforced by the existing streaming auditor. File identity, size, modification time, and on Unix ctime are checked across the audit.

The integration regression constructs the observed file through `stage_pinned_observed_census`, then compares it with a distinct digest-pinned fixture. Negative tests cover wrong expected digests, missing/unexpected IDs, same-file and hardlink aliases, symlinks, invalid namespace, empty input and truncated records.

## Exact-head qualification

- Initial source `eec0ba4691de249319fe15d218ecee0b7336129a` failed formatting; corrected source `8ebe15b251c9d5ebe08275d7f0fcd62aea0cc90d` passed Rust and Platform CI.
- Complete staged-event integration at **`450bc3c09250e04661cb3b159b7dfd5ad78b6044`** passed [Rust workspace CI 38626247033](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38626247033) (format, Clippy, workspace tests) and [Platform shells CI 38626247236](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38626247236), both for the exact SHA.

## Trust boundaries and next work

A matching caller-provided digest is **not publisher authentication**. Neither identical scope labels nor different file inodes prove that the expected census was independently sourced or represents the same publication cutoff. Synthetic semantic events do not establish actual XML/bzip2 integration. The file-metadata checks do not eliminate malicious same-user concurrent mutation of writable files; staging must be controlled.

Next: obtain an independently authenticated and cutoff-compatible expected census; verify actual compressed members, decode bzip2/XML into pinned semantic events, audit every shard, then qualify renderer fidelity and mandatory offline media. **SRC-003 remains unchecked.**


## Audited no-overwrite publication

The follow-up `stage_and_audit_pinned_census` connects the private pinned-event staging API to the digest-pinned expected-census gate and only promotes a final filename after a successful audit. It uses an internal pending name, rejects unsafe final filenames, hard-links the qualified bytes to a no-overwrite destination, removes pending files on mismatch/failure, and syncs the containing directory on Unix. Tests cover a complete staged publication, existing-destination refusal, mismatched independent census and pending-file cleanup. The source remains an untrusted intermediate until both event ingestion and expected-census reconciliation succeed.

- Exact source SHA **`f88ae7fd74edfaaeea22d0600e1f7c75e560624d`** passed [Rust workspace CI 38627603898](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38627603898) (format, strict Clippy and workspace tests) and [Platform shells CI 38627603589](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38627603589). An initial rustfmt-only failure was corrected.
- **Not yet complete:** the independently authenticated expected census and real verified bzip2/XML-to-events feed still must be provided; this API alone cannot qualify Wikimedia completeness or offline reader fidelity.
