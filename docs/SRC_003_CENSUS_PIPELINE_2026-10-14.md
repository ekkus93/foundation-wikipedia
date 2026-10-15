# SRC-003 — Verified pinned-ingest census integration (partial)

Date: 2026-10-14  
Status: Qualified partial implementation; SRC-003 remains unchecked.

## Implemented

- `public_dump_census_export::ObservedCensusWriter` streams the page IDs observed in one selected namespace to a caller-controlled staging sink.
- `public_dump_census_ingest::PinnedObservedCensus` now connects the verified-member-order and page/revision metadata validators to that exporter. The census is only returned once every declared member was closed and the ingest and writer completed successfully.
- The bridge poisons itself permanently after malformed page events, missing/out-of-range/duplicate IDs, wrong member order, or an output writer failure. Failed sessions cannot return a successful census artifact. Valid events from other namespaces still participate in source ordering but are not exported for the selected namespace.
- Regression tests exercise two pinned shards, cross-namespace filtering, missing final members, duplicate IDs, writer failures, and terminal invalid states.

## Qualification

- Formatting repair commit: `83f2ae1d49fb86a1536ea4bf5d12975f0327acc5`. [Rust workspace CI 38609403917](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38609403917) and [Platform shells CI 38609403818](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38609403818) passed.
- Integration code at exact SHA `7248bff5f2cab122548376958c2044ab432bde35` passed [Rust workspace CI 38610122643](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610122643) (format, strict lint and tests) and [Platform shells CI 38610122669](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610122669).
- Both changes were committed directly on `master` using Ralph Bridge v0.6.1; no per-task branch or PR.

## Not yet demonstrated

The caller must still discover/authenticate the original Wikimedia publication, verify each actual compressed member's digest, securely decode bzip2 and well-formed XML into events, and stage the output privately before atomic publication. This *observed* census is **not an independent expected census** and cannot prove that upstream pages were omitted. Full Parsoid/MediaWiki rendering, mandatory offline visuals with attribution, pack installation, and real desktop/Android acceptance remain outstanding. Do not mark SRC-003 complete.

## Additional fail-closed completion tests

Exact source SHA `ce48afe4a9f04434f1f761f5af30b43c1c6711bf` adds integration regressions for a completely validated multi-member stream with **no pages in the selected namespace**, and for a staging writer that accepts all page bytes but fails final `flush()`. Both must return an error without yielding a publishable output handle. Formatting correction of the first test-only commit `5de04bb6d14ffdce2d2f34f68d52dddeac70e278` preceded qualification.

- [Rust workspace CI 38610955304](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610955304) — passed, exact source SHA.
- [Platform shells CI 38610955599](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38610955599) — passed, exact source SHA.

These tests qualify fail-closed completion behavior only; staged files still require caller-managed deletion/atomic promotion and independently authenticated source data.


## Private staged observed-census publication

Exact source SHA: `43f17e9fe80edb07ea74ded8792db7c3182b60d6`.

- [Rust workspace CI 38616793402](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38616793402) — success (format, Clippy, unit tests), exact SHA.
- [Platform shells CI 38616793052](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38616793052) — success, exact SHA.

`public_dump_census_ingest::stage_pinned_observed_census` writes to a private, restrictive-permission temporary file, requires all pinned semantic events and a successful final flush, syncs the output, and atomically hard-links it to a no-overwrite destination. Failure removes the temporary file; an existing published census is not replaced. Tests cover successful multi-member output, duplicate destination, incomplete/duplicate source events, invalid filenames, and symlink staging root rejection.

**Limits:** the caller must use a private directory that cannot be concurrently modified by an adversary. The callback is responsible for supplying semantic events decoded from independently checksum-verified compressed XML members. The function does not authenticate Wikimedia, decode bzip2/XML, compare an independently sourced expected census, render HTML or publish a qualified offline pack. SRC-003 stays unchecked.

## Staging directory permission hardening

Exact source SHA: `581eeb2eb45ea080a58f24e9dce311a47f4094fd`. The staging API now refuses Unix directory roots readable or writable by group/other users (permission bits `0o077`), rather than relying only on caller documentation. Tests create a private `0700` staging root and verify that a `0755` root is rejected before any output is written. The requirement protects temporary census contents and destination names but does not replace upstream authenticity or the independent expected census.

- [Rust workspace CI 38618062528](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38618062528) — passed at exact source SHA.
- [Platform shells CI 38618062607](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38618062607) — passed at exact source SHA.

`SRC-003` remains unchecked.

## Public-dump staging permission boundary

Qualified on exact master `a21ba192021a15302b62dcc73ceca1e98620b4d9`:

- [Rust workspace CI 38631578259](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38631578259) — success.
- [Platform shells CI 38631578520](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38631578520) — success.

`public_dump_stage` now rejects Unix staging roots and nested member directories writable by group or other users (permission bits `0o022`) before accepting staged bytes or verifying a member tree. A deterministic regression covers rejection before writing, refusal to verify an unsafe root, rejection of a group-writable nested directory, and recovery after permissions are restricted. World-readable but non-writable directories are allowed because published dump bytes are public; source staging is still not safe against a privileged or same-owner concurrent writer. This closes a local path-substitution exposure, **not** the SRC-003 importer, upstream authentication, bzip2/XML parsing, independent census, renderer fidelity, or mandatory-media acceptance. `SRC-003` remains unchecked.

## Diagnostic bzip2/XML observed-census decoder (2026-10-14)

Added `scripts/public_dump_xml_census.py` and `scripts/test_public_dump_xml_census.py` as a **Python-side diagnostic**, not a replacement for the portable Rust importer. The decoder reuses the staged-source SHA-256 verifier, decompresses real bzip2 bytes, uses streaming ElementTree XML events, validates outer page and revision identity (not contributor IDs), enforces sorted shard page ranges and namespace selection, rejects DTD declarations, and publishes an LF-delimited census by no-overwrite hard link only after all members decode. The test suite covers valid multi-member input, corrupted compressed data, truncated bzip2 even with matching supplied digest, malformed XML, duplicate/out-of-range page IDs, namespace filtering, overlapping ranges and no-overwrite publication.

**Limits:** The manifest's `completed` and `source_url` labels do not authenticate the publisher; the digest must be independently trusted. The script supports the `enwiki-YYYY-MM-DD-pFIRSTpLAST.xml.bz2` filename family only, has a 16-GiB decompressed-member cap, and is not qualified against real upstream dumps. It does not produce article wikitext, canonical records, revision-pinned rendered HTML, an independent expected census, mandatory offline media, or a pack. It is a narrow step toward SRC-003 and must not be described as complete source qualification.

Run: `python3 -m unittest discover -s scripts -p 'test_public_dump_xml_census.py'`. The Rust workflow's existing `python3 -m unittest discover -s scripts` includes these tests.

## Same-descriptor compressed-byte revalidation

The diagnostic decoder now opens each staged member with no-follow semantics, rejects nonregular and hard-linked files, and hashes the exact compressed bytes consumed by bzip2. It compares their digest, byte length and file metadata against the previously verified manifest after full XML parsing, preventing a replacement between initial staging verification and decode from silently yielding a publishable census. A deterministic replacement-after-preverification test is included. This still does not establish independent Wikimedia publisher authentication or expected page census.

## Exact-head qualification of diagnostic XML decoding

- Implementation commit `b1fa28cb1e93b76117e2e4cb7049d8c8bb2ec8cc`: [Rust workspace CI 38633415979](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38633415979) and [Platform shells CI 38633416286](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38633416286) both passed.
- Same-descriptor source revalidation commit `2557eb055f212a7511d56657e50e2035081ad278`: [Rust workspace CI 38633971602](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38633971602) and [Platform shells CI 38633971483](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38633971483) both passed. The Rust workflow runs Python unittest discovery, including seven diagnostic XML tests.

These results establish **synthetic fixture regression qualification**, not successful parsing of an official dump generation or complete public-dump fallback acceptance.

## XML page-size and namespace hardening (2026-10-14)

- At `12add970c0606972d80d12e97b28557c31da2d48`, the diagnostic XML decoder limits each open page subtree to approximately 32 MiB of decompressed input. A synthetic oversized page is rejected without publishing a census. [Rust CI 38642126162](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38642126162) and [Platform CI 38642126013](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38642126013) passed at this exact SHA. The bound allows one read buffer of overshoot and is not a formal peak-RSS guarantee.
- At `1af4d7bb1dcc0fe767d4cd3827a992b96db24453`, the decoder accepts the MediaWiki export XML namespace while rejecting foreign root/page namespaces and requiring identity children in the correct namespace. [Rust CI 38642919102](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38642919102) and [Platform CI 38642919205](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38642919205) passed at this exact SHA. An intermediate test failure from an incorrect expected newline was fixed before qualification.

These changes harden the diagnostic parser but do not establish official source authenticity, independent census completeness, portable Rust importer support, rendering fidelity, or offline-pack acceptance. SRC-003 remains unchecked.

## Revision timestamp and XML nesting qualification (2026-10-14)

- At `4f58ee1b5e71861b2c1472129e11dbb11fa975e7`, revision timestamps are checked as real UTC calendar dates, not merely as matching a date/time string shape. Synthetic invalid day, month and hour fixtures must fail without publishing output. [Rust CI 38643722732](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38643722732) and [Platform CI 38643722810](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38643722810) passed.
- At `e15ddc08215a813c50852a17f500129a824b531e`, the diagnostic XML parser rejects nesting deeper than 128 elements, with a regression ensuring no census is published for excessively nested input. [Rust CI 38643834665](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38643834665) and [Platform CI 38643834676](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38643834676) passed at the exact head.

Both are bounded diagnostic safety improvements, not evidence of official source authenticity, independent census completeness, renderer fidelity, a portable Rust importer, or an offline pack. SRC-003 remains unchecked.

## Optional digest-pinned expected census gate (2026-10-14)

The Python diagnostic importer now accepts `--expected-census PATH --expected-sha256 HEX` together. After all bzip2/XML members are validated and the observed census is fsynced, it reads the separately supplied expected census as bounded LF-terminated positive decimal page IDs. Both streams must agree exactly, with strict increasing order and no missing/extra IDs. The expected input must be a non-symlink regular file, must not alias the pending output, and must match its supplied SHA-256 and stable file metadata. An invalid comparison never publishes the pending file.

- Implementation SHA: `007b16e023ce30e31851216ce147e2dc6c1c4dbc`.
- [Rust workspace CI 38649085525](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38649085525) — passed, includes diagnostic Python regression discovery.
- [Platform shells CI 38649085131](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38649085131) — passed, same exact SHA.
- Regression cases include equal multi-page census, missing/extra IDs, reordered IDs, malformed records, digest mismatch, missing digest and symlinked expected census.

**Crucial limit:** A checksum is only an integrity pin. Neither the expected census's independent origin nor its exact source-generation cutoff/namespace is established by the tool; the caller must separately authenticate both. Without the optional gate, the script still emits only an observed census. This does not qualify actual Wikimedia dump acquisition, a portable Rust XML importer, HTML rendering or mandatory offline media. `SRC-003` remains unchecked.

## Non-page XML memory bound and root structure (2026-10-14)

- The Python diagnostic decoder now enforces an 8 MiB cap on decompressed XML accumulated between page elements (notably untrusted `siteinfo` metadata), refuses unknown root-level XML elements, and rejects duplicate or misplaced `siteinfo` elements. It releases processed `siteinfo` subtrees to avoid retaining metadata in the root. The existing 32 MiB per-page bound and 128-level nesting bound still apply.
- Deterministic regression coverage includes valid `siteinfo`, duplicate/misplaced metadata, unrecognized root children, and oversized metadata with no census published.
- Initial SHA `9d40610d8a24ad4df7603a0812f6b51c13d87e05` failed Python regression tests because one byte-literal newline assertion and one pre-existing error-message expectation were inconsistent. Both were fixed in exact code SHA **`57187f08ad681e9555aa3fee62b85ed6abe856e5`**.
- [Rust workspace CI 38657955558](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38657955558) — success at the corrected SHA, including diagnostic Python tests.
- [Platform shells CI 38657955492](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38657955492) — success at the same SHA.

**Limitations:** This protects a diagnostic importer from unbounded non-page XML accumulation; it does not establish independently authenticated Wikimedia source generation, an independent complete census, portable Rust importer integration, full renderer fidelity, mandatory offline media or end-to-end pack acceptance. **SRC-003 remains unchecked**. The canonical TODO was restored on master at `f80a53a32918eeb7de74c6885a87a689bdc13ec1` using the original `007b16e023ce30e31851216ce147e2dc6c1c4dbc` file blob, retaining all 1,536 lines.

## Diagnostic source staging inventory qualification

At code SHA `4ceeebc6f5d3311266046748feab05e9ae2af894`, source staging rejects undeclared files, symlinks, subdirectories, symlinked ancestor paths, and Unix staging directories writable by other users. Regression tests cover each rejection and verify normal pinned members still pass. This is a local diagnostic safety improvement, not proof of source authenticity or complete Wikimedia coverage.

- Rust workspace CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38660466535 (passed).
- Platform shells CI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38660466246 (passed).

SRC-003 remains unchecked pending independent publication authentication, completeness verification, portable Rust importing, rendering fidelity, and required offline media.

## Compressed-source declaration byte budgets (2026-10-14)

At code SHA `c258fe8f314625ea4fb03760516245b9d857beaa`, the Python diagnostic staging verifier rejects manifest-declared compressed members exceeding 64 GiB each or a cumulative 4 TiB across members. The check occurs before hashing the offending member. Synthetic tests lower each budget to exercise individual and aggregate rejection and confirm that valid pinned input still passes. These are **diagnostic safety ceilings**, not evidence of Wikimedia publication authenticity or full-dump completeness.

- [Rust workspace CI 38666223461](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38666223461) — passed at the exact code SHA, including Python unittest discovery.
- [Platform shells CI 38666223418](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38666223418) — passed at the same SHA.

The byte ceilings do not establish a source-generation cutoff, independent expected census, portable Rust XML importer, renderer fidelity or required offline media. **SRC-003 remains unchecked.**
