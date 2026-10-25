# SRC-003 — Expected-census digest preflight before dump decoding

**Date:** 2026-10-24  
**Status:** Scoped fail-fast integrity improvement qualified; SRC-003 remains incomplete.

## Change

The `wiki-pack audit-public-dump` command already preflights all source
members and refuses occupied census output names before decompressing a
Wikimedia dump generation. However, it previously discovered a malformed,
stale or incorrect independently supplied **expected census SHA-256 pin**
only after potentially decompressing every member.

The shared `stage_and_audit_pinned_census` gate now preflights the
expected census file before creating a pending census or calling the native
bzip2/XML member opener. It validates 64-character hexadecimal SHA-256
syntax, opens a regular nonempty bounded file through the existing
symlink/hardlink-safe reader, hashes its complete bytes, checks the
caller-supplied digest and confirms that the file's metadata did not
change during that hash.

This preflight **does not replace** the post-ingestion audit. After all
inventory-verified shards have been decoded, the existing audit
independently reopens the expected file, validates its digest, compares
the complete namespace page-ID sets, rehashes expected bytes, checks
source metadata and publishes the observed census atomically only on
success. A concurrent mutation remains subject to the final gate.

## Regression evidence

A new native audit regression supplies an invalid expected SHA-256 and a
well-formed but mismatched digest. Both fail before opening a single
native member, and neither creates a pending or final census. The valid
expected digest remains accepted by preflight; the existing two-shard
real-bzip2 roundtrip continues to verify the full audit.

- Initial code `288d0536285faaddb312c9664b1d4c39b0279700` failed
  [Rust rustfmt CI 40159447555](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40159447555).
- Corrected source `5e0ed6d096fb9f10c456e0e32f684338c5c9e5a1` passed
  [Rust workspace 40159526929](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40159526929)
  and [Platform shells 40159526963](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40159526963).

## Limits and remaining acceptance

The expected census and its SHA-256 are still **caller supplied**. A
matching hash does not establish Wikimedia publisher authenticity,
independent census origin, compatible snapshot cutoff or full source
coverage. The command remains a local integrity/census diagnostic, not
a revision-matched HTML/media importer or pack installer. SRC-003 and
downstream offline/device acceptance must remain unchecked.
