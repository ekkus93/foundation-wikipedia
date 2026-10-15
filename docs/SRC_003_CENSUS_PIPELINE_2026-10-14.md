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

## Bounded CLI manifest loading (2026-10-14)

At code SHA `9a28b097f321dda58bc74f87f78a68c854931e27`, the diagnostic source-verification CLI reads a manifest only from a regular, single-linked file descriptor opened with no-follow/nonblocking flags where supported. It rejects empty or over-32-MiB manifests before JSON decoding, rejects malformed/non-object JSON, and compares file identity, size and timestamps before/after its bounded read. Synthetic tests cover valid JSON, oversized files, malformed/non-object JSON, and symlinked manifests on platforms with `O_NOFOLLOW`.

- [Rust workspace CI 38666449846](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38666449846) — passed at exact code SHA, including diagnostic Python tests.
- [Platform shells CI 38666449757](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38666449757) — passed at the same SHA.

These controls apply to the diagnostic CLI input boundary, not to independently authenticated source publication, complete page census, portable Rust XML decoding, Parsoid rendering or mandatory offline media. **SRC-003 remains unchecked.**

## Bounded decimal identity parsing (2026-10-14)

Code SHA `d3811b8099110aa7716e75eb527e00a2a3a188c6` rejects XML page/revision IDs longer than 19 decimal digits, namespace IDs longer than 10 digits or beyond the signed 32-bit range, and shard range identifiers longer than 19 digits before attempting Python integer conversion. This prevents untrusted multi-thousand-digit values from surfacing interpreter-specific integer-conversion errors outside the diagnostic importer's expected `DumpDecodeError` contract. Tests exercise all three XML identity fields, oversized shard names, and invalid namespace selection, requiring no partial census publication.

- [Rust workspace CI 38666706953](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38666706953) — passed at exact code SHA, including Python regression tests.
- [Platform shells CI 38666706916](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38666706916) — passed at the same SHA.
- The preceding documentation-only SHA `55fcd1e365376078e0310e4203a81818b54db976` had a failing Rust CI run 38666642949 in an unrelated `online_curl::tests::caps_streamed_body_without_trusting_content_length` subprocess test (`SpawnFailed` versus expected `ResponseTooLarge`). The next exact-head Rust run above passed without modifying that transport code. This is recorded as a potentially transient test failure, not evidence that the earlier SHA qualified.

The diagnostic importer's bounds are not a substitute for independent publisher authentication, complete source coverage, portable Rust bzip2/XML parsing, Parsoid rendering fidelity, or required offline media. **SRC-003 remains unchecked.**

## Complete manifest preflight before member reads (2026-10-14)

At source SHA `70cbc464dca92c918a9996e498d076825b0474c9`, the Python diagnostic staging verifier validates **all** declared member names, checksums, types, duplicate identities and byte budgets before opening or hashing any staged member. Regression SHA `0b23cea78aedae455a63ade31485252ef7f6f923` adds a late-invalid-member test: even when an earlier staged member is absent, invalid metadata in a later declaration is rejected first. This avoids avoidable expensive hashing before detecting a malformed or oversized manifest.

- [Rust workspace CI 38667126349](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38667126349) — **passed on run attempt 2**, exact SHA `0b23cea78aedae455a63ade31485252ef7f6f923`, including Python tests.
- [Platform shells CI 38667126285](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38667126285) — passed at the same exact SHA.
- Initial Rust run attempt 1 failed in the unrelated `online_curl::tests::drains_large_stderr_and_preserves_final_http_status` fixture with `SpawnFailed`. Ralph Bridge reran failed jobs at the same SHA; attempt 2 passed. The subprocess test flake has **not** been fixed by this source-staging change.

This remains a partial diagnostic-source safety improvement, not independently authenticated Wikimedia acquisition, completeness, portable Rust XML import, renderer fidelity, or mandatory offline-media acceptance. **SRC-003 remains unchecked.**

## Bounded manifest loading in XML census CLI (2026-10-14)

At exact code SHA `afed681261efb2f987cf3fcd96e558025462b447`, the Python diagnostic XML/bzip2 census CLI now reuses `verify_source_staging.load_source_manifest` rather than unbounded `Path.read_text()` / `json.loads()`. The shared loader requires a stable, regular, single-linked descriptor opened without following symlinks where supported, bounds input to 32 MiB, and rejects malformed/non-object JSON. New CLI regressions exercise a valid manifest, symlinked input, oversized manifest and a non-object manifest; rejected inputs leave no published census.

- [Rust workspace CI 38667942405](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38667942405) — **passed** on exact code SHA (including Python unittest discovery).
- [Platform shells CI 38667942367](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38667942367) — **passed** on the same SHA.

This closes only a diagnostic CLI input boundary. Manifest fields and digests alone do not authenticate the upstream publisher or independent census, and the portable Rust importer, rendering fidelity, mandatory media and end-to-end pack acceptance remain outstanding. **SRC-003 remains unchecked.**

## Reject duplicate JSON object fields in source manifests (2026-10-14)

Code SHA `7290668e63bea18952c6e981ded8fc87fdcd6568` makes `load_source_manifest` reject duplicate JSON keys at **every object depth**, including nested source-member metadata. This prevents conflicting `project`, `name`, `bytes` or `sha256` values from being silently interpreted using JSON last-key-wins behavior. The loader raises the typed `SourceVerificationError`; tests cover root-level and nested conflicting declarations. Both the source-verification CLI and the XML census CLI use this shared loader.

- [Rust workspace CI 38668294006](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38668294006) — passed on exact code SHA, including Python regression tests.
- [Platform shells CI 38668294054](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38668294054) — passed on the same exact SHA.

This is input-ambiguity hardening only; it does not authenticate an official dump, prove independent census completeness, qualify a portable Rust importer, produce Parsoid-grade HTML or include required offline media. **SRC-003 remains unchecked.**

## Fail-closed manifest JSON constants and structural depth (2026-10-14)

The shared diagnostic manifest loader rejects Python's non-RFC JSON extensions `NaN`, `Infinity` and `-Infinity` anywhere in a source manifest. It also deterministically limits JSON array/object nesting to **128 levels**, scanning before decoding while correctly ignoring brackets inside quoted strings and escaped quotes. Overly nested manifests fail with a typed `SourceVerificationError` rather than depending on interpreter recursion limits. Both the checksum-verifier and XML-census CLIs reuse this loader.

- Initial code SHA `778aeac58a001a7a4352059a0d30731325eef06d` failed [Rust CI 38668631337](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38668631337), because a test assumed Python would reject a 2,000-level array with a recursion error, whereas that interpreter decoded the array before rejecting its non-object top level. This SHA is **not** qualified.
- Corrected code SHA **`bc038b5e8af6800b67a751fc3ad3278a536ddb4c`** adds a deterministic pre-parse depth bound and a test proving that long bracket sequences inside a JSON string remain accepted.
- [Rust workspace CI 38668982354](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38668982354) — **passed** on the corrected exact SHA, including Python regression discovery.
- [Platform shells CI 38668982370](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38668982370) — **passed** on the same SHA.

This is strict diagnostic input handling, not independently authenticated Wikimedia acquisition, independent page-ID completeness, production Rust bzip2/XML import, renderer fidelity or required offline-media acceptance. **SRC-003 remains unchecked.**

## Optional revision-scoped diagnostic metadata records (2026-10-14)

Exact code SHA **`93395071509ccbabaa43895d9df8d62c83b88124`** adds an opt-in `--format revision-jsonl` mode to `scripts/public_dump_xml_census.py`; the prior LF-separated page-ID census remains the default (`--format ids`). For the selected namespace, the diagnostic XML decoder can now emit deterministic UTF-8 JSON Lines records with `page_id`, `namespace`, `title`, `revision_id`, `timestamp` and nullable `redirect_target` from the same checksum-verified compressed member consumed by the census path. Redirect target attributes are validated, and duplicate/invalid redirect entries fail closed before private no-overwrite publication. A caller cannot apply the independent expected-ID-census gate to JSONL records; it is explicitly restricted to the ID format.

- [Rust workspace CI 38671295631](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38671295631) — **passed** at exact code SHA, including deterministic Python fixtures for Unicode titles, revision IDs, redirects and rejected malformed redirects.
- [Platform shells CI 38671295656](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38671295656) — **passed** at the same SHA.

**Scope:** These are outer XML identities only; they are not imported canonical article blocks, revision-pinned rendered HTML, the article's wikitext, authenticated source publication, an independent expected census, or required images/math and attribution. The production portable Rust importer and end-to-end offline packs remain unqualified. **SRC-003 remains unchecked.**

## Diagnostic revision wikitext retention and record provenance (2026-10-14)

- **Wikitext extraction:** Code SHA `90a4cf9ab966d8e6bfd8158a3cde2dd190744958` adds opt-in `--format revision-content-jsonl` to the verified compressed XML diagnostic importer. The parser retains one direct `revision/text` or modern `revision/slots/slot[@role=main]/text` value, rejects absent/deleted/ambiguous content, and includes a SHA-256 of the UTF-8 text after XML decoding. The default census and identity-only JSONL modes remain available. [Rust workspace 38671832600](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38671832600) and [Platform shells 38671832797](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38671832797) both passed exact-head checks.
- **Scoped record provenance:** Correction code SHA **`ddca0b8ffd397c929ec0fdc9940282201bd92c7e`** adds a schema version, explicit `project` and `generation_id` labels from the declared source manifest to JSONL records; rejects control characters in page titles and nested child elements in revision-text payloads. Tests cover Unicode source text, inline content and main-slot forms, duplicate/unavailable wikitext, and actual control characters. [Rust workspace 38672759662](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38672759662) and [Platform shells 38672759723](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38672759723) passed at this exact head.
- **Intermediate test failure:** SHA `4a1718d9c9e989222d0713ab60fc4db5ec196338` failed [Rust CI 38672279298](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38672279298) because the control-character test provided literal backslash escapes instead of actual newline, tab and carriage-return characters. The corrected fixtures are qualified only at the later SHA above; the failed SHA is not qualified.

**Limitations:** The JSONL schema labels are caller-declared provenance, **not** independently authenticated Wikimedia publisher identity. Extracted text is XML-decoded and may not be byte-identical to raw wikitext; its SHA-256 is calculated over the decoded UTF-8 string. These records are not canonical Articles, template/Lua/math expansion, revision-matched Parsoid HTML, independent page-census completeness, mandatory media or offline packs. A production portable Rust XML/bzip2 importer remains outstanding. **SRC-003 remains unchecked.**

## Pre-publication diagnostic revision JSONL self-audit (2026-10-14)

At exact code SHA **`beb9c11d2b4290cec402c4c1082f65b8c99d09a1`**, `stage_observed_census` now calls the independently implemented `audit_public_dump_revision_jsonl.audit_revision_jsonl` for both `revision-jsonl` and `revision-content-jsonl` outputs **before** hard-link publication. The auditor validates schema version, declared project/generation scope, namespace, monotonically increasing page IDs, per-record revision/timestamp/redirect data and (for content records) the decoded-text digest. An audit exception or selected-record count mismatch aborts publication and removes the pending temporary file. The default LF page-ID census and its separately pinned expected-census gate remain unchanged.

- [Rust workspace CI 38674391914](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38674391914) — **passed** at exact code SHA, including Python tests for simulated self-audit exceptions, wrong counts and no published output.
- [Platform shells CI 38674391936](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38674391936) — **passed** at the same SHA.

These checks establish consistency of the diagnostic output with the declared source manifest, **not** independent Wikimedia authenticity or generation completeness. Wikitext still requires a production Rust importer, revision-pinned Parsoid/MediaWiki rendering, mandatory offline media/attribution and full pack acceptance. **SRC-003 remains unchecked.**

## Independent expected-ID census gate for diagnostic revision exports (2026-10-14)

At exact source SHA **`601d2b1f3f0fcf143f2bb8cfa07397ad0ccc1147`**, the optional digest-pinned expected page-ID census gate now applies to **all three** diagnostic export modes: `ids`, `revision-jsonl`, and `revision-content-jsonl`. For JSONL modes, the generated revision records are self-audited first; then the separately supplied LF-delimited, strictly increasing expected page-ID census is compared record-by-record against the JSONL page identities **before** no-overwrite publication. Invalid, missing or extra IDs and digest mismatches leave no published artifact. The existing source-member digest and scope checks remain mandatory.

- [Rust workspace CI 38678104016](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38678104016) — **passed** on exact source SHA, including Python regression discovery.
- [Platform shells CI 38678103940](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38678103940) — **passed** on the same source SHA.
- Deterministic fixtures exercise both revision JSONL modes, exact census agreement, missing/extra/different expected IDs, and expected-digest mismatch.

**Limitations:** The expected census is caller-provided and digest-pinned, not independently authenticated or proven to describe the same Wikimedia publication generation and namespace. This extends the optional local completeness comparison to revision-bearing diagnostic exports; it does not establish publisher authenticity, a portable Rust XML importer, Parsoid/MediaWiki rendering fidelity, mandatory offline visuals, or pack acceptance. **SRC-003 remains unchecked.**

## Diagnostic revision-text declared byte-length validation (2026-10-14)

Source implementation SHA **`7b11a71dc24ff883ff0f630003386ee2bae747f9`** verifies
the optional MediaWiki XML `<text bytes="...">` declaration against the
actual UTF-8 byte length of decoded wikitext in `revision-content-jsonl`
mode. A malformed or contradictory declaration aborts private staging before
publication. Legacy `revision/text` and modern main-slot `slot/text` are
both covered, including Unicode text, rejected leading-zero, negative,
nonnumeric and oversized declarations, and no-overwrite failure behavior.
Absent `bytes` is permitted for older XML exports.

- [Rust workspace CI 38679774228](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38679774228) — passed on exact source SHA, including Python unittests.
- [Platform shells CI 38679774230](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38679774230) — passed at the same exact SHA.

This is internal metadata consistency for a **diagnostic Python export**,
not independent publisher authentication, a qualified portable Rust
bzip2/XML importer, complete page-census verification, Parsoid rendering,
mandatory offline media, or end-to-end offline pack acceptance.
**SRC-003 remains unchecked.**

## Portable Rust diagnostic revision-content JSONL ingestion boundary (2026-10-14)

New `wiki-source::public_dump_revision_jsonl::DiagnosticRevisionLines` consumes
LF-terminated diagnostic `revision-content-jsonl` records incrementally through
a caller-supplied `BufRead`. It has explicit 128 MiB encoded-record, 32 MiB
decoded-wikitext and 100-million-record limits. Serde rejects duplicate,
unknown, missing and mistyped JSON fields; Rust checks the declared project,
generation, namespace, strict page-ID order, valid revision IDs and UTC
calendar dates, bounded titles, optional redirect targets and the decoded
UTF-8 wikitext SHA-256. Errors permanently poison the iterator, and
truncated final records fail rather than becoming successful EOF. Callers
must consume the iterator to EOF and handle any error before activating
downstream content. Regression tests exercise Unicode, small buffered reads,
wrong scope, duplicate fields, corrupted digest, date drift, ID order and
truncated records.

- First code commit `e8cc1089cb0bef02871ae0ae5c2b0e12b4f795cd` failed Rust format CI
  [38681041180](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38681041180);
  it is **not** qualified.
- Corrected code SHA **`ef5d0a8697eea0bd50d7cf287341ea9530a5e66f`** passed
  [Rust workspace CI 38681257988](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38681257988)
  (format, Clippy, tests and configured checks) and
  [Platform shells CI 38681258070](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38681258070).

**Boundary:** This is a portable Rust **consumer of previously decoded diagnostic
JSONL**, not the Rust bzip2/XML source decoder, a signed or independently
authenticated Wikimedia publication, a cross-checked independent page census,
a revision-pinned Parsoid renderer, required-media pack assembly, or an
end-to-end Android/desktop qualification. No official/source-complete claim is
made, and **SRC-003 stays unchecked**.

## Rust revision-content records versus independent ID-census stream (2026-10-14)

The portable Rust `wiki-source::public_dump_revision_jsonl::audit_revision_content_census`
function now composes the validated diagnostic `revision-content-jsonl` record
iterator with the existing bounded `PageIdLines` and
`audit_page_coverage_stream` implementation. It rejects missing/extra
page IDs, malformed/truncated or unordered expected census lines, corrupted
revision-content digests, and invalid record provenance. It yields coverage
evidence only after successful consumption of both streams. The function does
**not** assert that a caller-supplied census is independently authenticated;
official publisher and generation cutoff still require external verification.

- Initial code SHA `2e2347005990cbe7a5aa6e32203dd1948b5de15c` failed Rust format CI
  [38681773970](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38681773970);
  do not treat it as qualified.
- Corrected SHA **`43840888134c5a34641c00d2af5d7ab699edc48a`** passed
  [Rust workspace CI 38682169467](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38682169467)
  (format, Clippy, tests and configured gates) and
  [Platform shells CI 38682169453](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38682169453)
  at this exact source head.

**Outstanding:** Authenticate a real upstream Wikimedia publication and
independent namespace-aware census; implement the production portable Rust
bzip2/XML importer and complete Parsoid-fidelity article/media normalization;
build and qualify complete offline packs on desktop and Android.
**SRC-003 remains unchecked**.
