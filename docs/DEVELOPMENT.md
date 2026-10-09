# Foundation Wikipedia development environment

This document describes the bootstrapped projects. Requirements and completion are governed by [SPEC](WIKIPEDIA_AI_READER_SPEC.md) and [TODO](WIKIPEDIA_AI_READER_TODO.md).

## Rust libraries and CLI

Rust **1.85.1** is pinned in the root `rust-toolchain.toml`. Requires Cargo, rustfmt and Clippy; no Wikimedia account, LLM or downloaded dataset is required for unit tests.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p wiki-pack -- --help
```

The nine library crates are platform independent and **must not** import Tauri or Android APIs. The CLI currently only prints bootstrap help; no packs are built.

## Desktop (Tauri 2 / React / TypeScript / Vite)

Requires Node.js **22**, npm, Rust **1.90.0** for native Tauri (pinned in `desktop/src-tauri/rust-toolchain.toml`), and Linux WebKitGTK 4.1 development dependencies for native builds.

On Ubuntu 24.04:

```sh
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev
cd desktop
npm install
npm run lint
npm run build
npm run tauri dev
```

To check native Tauri while at repo root (avoid accidental root Rust 1.85 override):

```sh
cargo +1.90.0 check --manifest-path desktop/src-tauri/Cargo.toml
```

The React shell displays a static placeholder in Wikipedia-style typography and a nonfunctional action hub. Real article fetching and WebView bridge are future tasks. Desktop npm dependencies are directly pinned; a committed transitive lockfile and installer qualification remain TODO work.

## Android (Kotlin / Compose / WebView)

Requires **JDK 17**, **Gradle 8.11.1**, Android SDK Platform 35 and build-tools 35.0.0. The APK uses minimum API 26. Install Gradle and Android SDK separately; the repository does not yet include Gradle wrapper artifacts.

```sh
cd android
gradle :app:assembleDebug :app:testDebugUnitTest --no-daemon
```

Debug APK expected at `android/app/build/outputs/apk/debug/app-debug.apk` after a successful build. This is not proof of real-device installation until tested. Android shell shows a static local article placeholder with JavaScript disabled and file/content access blocked. No Internet permission is requested yet.

### Shared Rust arm64 build stub

The `wiki-ffi` crate is configured as both `rlib` and `cdylib`. On a supported NDK host, install the pinned Rust target and use the explicit Gradle task:

```sh
rustup target add aarch64-linux-android
export ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/<installed-version>"
cd android
gradle :app:buildRustArm64
```

This builds `libwiki_ffi.so` using the NDK API 26 linker and copies it to the ignored `app/src/main/jniLibs/arm64-v8a/` directory. The native task is **opt-in**: normal APK assembly still builds a placeholder shell without Rust.

BOOT-002 also carries an opt-in UniFFI **code-generation stub**, not a runtime
bridge. The checked-in `crates/wiki-ffi/src/foundation_wikipedia.udl` currently
exports no callables; point `UNIFFI_BINDGEN` at an explicitly provisioned
generator and run `gradle :app:generateUniFfiKotlin`. Generated sources go
under `app/build/generated/source/uniffi` and are not compiled into the app.
Production UniFFI Rust scaffolding, versioned DTOs/errors, Kotlin compilation,
async/cancellation behavior and device-tested shared-core calls remain MOB-001.
See `android/README.md` for the exact boundary.


### Offline local-file source staging (SRC-001 partial)

A dump member obtained by other means can be staged without a network call,
using the same completed official dump-discovery report and upstream SHA-1
identity as the resumable HTTPS downloader:

```sh
python3 scripts/discover_public_dump.py enwiki > completed-report.json
python3 scripts/stage_local_dump.py completed-report.json \
  enwiki-YYYYMMDD-pages-articles-multistream.xml.bz2 \
  /path/to/verified-local-file.xml.bz2 /path/to/staging
```

The report must refer to a completed generation and contain the exact official
file URL, expected size and upstream SHA-1. The command validates the copied
bytes and emits a SHA-256 receipt, does not overwrite existing final files,
and does not modify an interrupted HTTP `.part` transfer. It does **not**
authenticate a caller-supplied report, install a snapshot or imply publisher
signature verification. The upstream SHA-1 is a legacy integrity check, not
a modern cryptographic authenticity guarantee. The local input must be a
regular non-symlink file; the staging destination must not be a symlink.


### Finalizing a complete public dump staging set

After all members from one completed official discovery report have been
downloaded or staged locally, derive a complete SHA-256 manifest from the
actual bytes (without accepting a partial source generation):

```sh
python3 scripts/finalize_public_dump_staging.py \
  completed-report.json /path/to/staging /path/to/source-manifest.json
python3 scripts/verify_source_staging.py \
  /path/to/source-manifest.json /path/to/staging
```

Finalization checks **every** declared member against the upstream size and
SHA-1 and rejects missing, corrupt, duplicate or symlinked files. The
output manifest is published without overwriting an existing manifest.
It is still **not authenticated publication metadata**: discovery-report
authenticity and legacy SHA-1 limitations remain separate trust concerns.
No snapshot activation or import is performed.


### Complete-generation staging (SRC-001 partial)

Use the generation orchestrator to stage **all** members declared by a
completed official discovery report, either via resumable HTTPS or from an
already-downloaded local directory:

```sh
python3 scripts/stage_public_dump_generation.py \
  completed-report.json /path/to/staging /path/to/source-manifest.json \
  --local-directory /path/to/downloaded-files
# OR
python3 scripts/stage_public_dump_generation.py \
  completed-report.json /path/to/staging /path/to/source-manifest.json \
  --user-agent 'FoundationWikipedia/0.1 (contact: you@example.org)'
```

Network mode additionally re-fetches official dump status (and SHA-1 sums as
needed) over HTTPS before staging any members, rejecting altered inventories or
checksums; if upstream metadata is unavailable it fails closed. Offline
--local-directory mode intentionally does not make network requests. For an
independent preflight check of an archived discovery report, use:

```sh
python3 scripts/verify_public_dump_publication.py completed-report.json
```

This revalidation binds the report to currently published official HTTPS
metadata, but upstream SHA-1 and TLS do **not** provide signed historical
publisher authentication. Do not claim verified publisher trust from it.

It validates the full inventory before copying, verifies each member against
the report's upstream SHA-1 and size, and publishes a SHA-256 manifest only
when every member has passed. Interrupted network transfers leave resumable
`.part` files; an incomplete generation never produces the final manifest.
It does not authenticate a caller-supplied discovery report, import articles,
or activate a snapshot. Run the separate staging verifier before ingestion.

## Preliminary verified public-dump XML import (SRC-003 partial)

After completing full source staging, raw wikitext revisions can be extracted
from a staged official pages XML/XML.bz2 member without activating any snapshot:

```sh
python3 scripts/import_public_dump_xml.py source-manifest.json \
  /path/to/staging enwiki-YYYYMMDD-pages-articles-multistream.xml.bz2 \
  /path/to/raw-revisions.ndjson
```

The default 1,000,000-page import budget is deliberately conservative and
would reject a full English-Wikipedia extraction. Pass `--max-pages 10000000`
(or another justified positive limit) for a large qualified dump, and set
`--max-page-bytes` up to 67108864 for unusually large individual pages.
The streaming XML reader also limits **total decompressed bytes across every
selected shard** to 256 GiB by default (`--max-decoded-bytes` adjusts this,
with a hard maximum of 4 TiB). The decoded-byte budget catches large ignored
metadata and decompression bombs even when individual pages are small. Both
quotas fail closed without publishing incomplete output; large builds require
sufficient staging/output disk space and are not yet device-qualified.

For a single *non-overlapping* dump product split across XML shards, specify
the first member as usual and repeat `--additional-member` for each further
shard (in desired output order). The importer verifies the complete staged
manifest, rejects duplicate page IDs across shards and atomically publishes
one combined NDJSON only after every selected member succeeds. Do **not**
combine alternative dump products (e.g. pages-articles and pages-meta-current)
whose article inventories overlap. The parser requires a MediaWiki export
namespace and rejects foreign-namespace elements; this is input validation,
not a substitute for provenance verification.

Each emitted raw revision record contains the exact `source_member`,
`source_member_url` and the locally verified `source_member_sha256`,
alongside its page/revision/content SHA-256. Member ordering is sorted for
reproducible NDJSON regardless of CLI argument order. These fields are traceable
claims about staged bytes, **not** independent publisher signatures or proof
of a semantically faithful rendered article.

The import verifies the **full staged manifest** before consuming input,
rejects XML doctypes/entities, duplicate page IDs, multi-revision records,
unbounded page text and malformed revision metadata. Output is atomically
published without clobbering an existing result. Raw wikitext is **not**
rendered article HTML; template/Lua/math expansion, revision joins, sanitized
rendering and actual pack installation are still incomplete.

## Newer public current-content SHA-256 export discovery (SRC-001 partial)

Wikimedia's separate current-content XML export publishes an official
`SHA256SUMS` inventory only when that monthly export is complete. Discover
recent completed exports with:

```sh
python3 scripts/discover_current_content_export.py enwiki --months 4 \
  > current-content-report.json
```

The resulting report contains each original, official source URL and upstream
SHA-256 checksum. Resume/download **every** published member, verify hashes,
and write a no-clobber complete staging manifest with:

```sh
python3 scripts/stage_current_content_export.py current-content-report.json \
  /path/to/current-content-staging /path/to/current-content-manifest.json
python3 scripts/verify_source_staging.py \
  /path/to/current-content-manifest.json /path/to/current-content-staging
```

Alternatively, use `--local-directory /path/to/downloaded-shards` for
pre-downloaded XML members stored under **flat basenames**. This mode copies
every file via a no-follow descriptor into a no-clobber staging target,
checks complete upstream SHA-256 and leaves unrelated HTTP `.part` files
untouched. It still requires fresh official Wikimedia SHA256SUMS metadata
over HTTPS; it is not an offline authentication or offline discovery mode.
`--local-directory` and `--mirror-base` are mutually exclusive.

To download from an explicitly configured HTTPS mirror instead of the
original member host, append `--mirror-base https://mirror.example.test/exports/`.
The mirror URL is validated before any I/O, redirects are rejected, and the
official Wikimedia SHA256SUMS inventory is **still retrieved directly from
Wikimedia** and pinned before contacting the mirror. A mirror cannot supply
expected checksums or change the canonical source URLs recorded in the
manifest. Corrupt or truncated mirror bytes fail closed with no manifest
publication. Existing resumable partials may contain mixed transport bytes;
only the complete official SHA-256 digest is treated as verified. This does
not imply a mirror is a verified publisher.

Wikimedia member downloads and explicit HTTPS mirror transfers retry HTTP 429
and 503 at most twice (three total attempts). Numeric `Retry-After` values
must be at most 30 seconds; missing headers use short bounded backoff.
Unexpected HTTP status, oversized delays, malformed headers or repeated
throttling terminate without publishing a partial manifest. No retry can
replace upstream SHA-256 verification.

The stager re-fetches and compares official SHA256SUMS metadata before any
member transfer, rejects altered member inventories, checks exact HTTPS URLs,
validates resume ranges and SHA-256, and refuses incomplete manifest publication.
Published member-relative paths and exact original URLs are retained in
modern staging manifests and in subsequent raw revision NDJSON records.
Unlike the legacy source layout, new current-content exports can contain
nested shard paths; the importer validates that the recorded URL matches
the exact SHA256SUMS project/generation and rejects forged origins. All this
is transport provenance, not an independent signed publisher identity.

It is not a snapshot importer or installed article reader; no publisher
signature or device acceptance has been established.

This separate read-only discovery adapter does not activate content;
member sizes and installed-snapshot integrity still require independent staged
byte verification. The HTTPS checksum inventory is not a signed publisher
manifest. Unlike the older dump-status route, the newer export can provide
SHA-256 directly. See Wikimedia documentation:
https://wikitech.wikimedia.org/wiki/MediaWiki_Content_File_Exports

## Public Wikimedia dump discovery (read-only metadata)

Use `python3 scripts/discover_public_dump.py enwiki` to enumerate the official
project index and select the newest fully terminal generation whose
`articlesmultistreamdump` job completed. This reads official HTTPS
`dumpstatus.json` metadata, rejects redirects, incomplete jobs, unsafe paths,
and malformed sizes/checksums, and prints the exact file URLs and reported
SHA-1/MD5 checksums. `--max-dates` bounds index scanning; no content is
downloaded. Run offline negative tests with
`python3 -m unittest discover -s scripts -p 'test_discover_public_dump.py'`.

**Trust boundary:** legacy upstream SHA-1/MD5 values are not SHA-256 or signed
publisher authentication. Missing upstream checksums are explicitly reported.
This helper does not generate an accepted SHA-256 staging manifest or activate
snapshots. Fetch/resume, authentic digest provenance, SHA-256 recomputation,
revision joins, and real dump-import qualification remain open SRC-001/003 work.

## Bytewise verification of staged Wikimedia source files

After separately obtaining completed official-source release metadata and
authentic expected member SHA-256 digests, use a JSON manifest containing
`project`, `generation_id`, `source_url`, `completed: true`, and `files`
(entries with `name`, `bytes`, `sha256`) to verify locally staged files:

```sh
python3 scripts/verify_source_staging.py source-manifest.json /path/to/staged-files
```

For public dump manifests, `source_url` must match the exact
`https://dumps.wikimedia.org/PROJECT/GENERATION/dumpstatus.json` identity,
or the exact `/PROJECT/GENERATION/` directory when used as an HTTPS transfer
base.
Unrelated paths, query parameters and identity mismatches fail closed. This
binds reported metadata to a consistent source identity **but cannot certify
that the caller-supplied SHA-256 values came from Wikimedia**.

The verifier rejects truncated or corrupted bytes, symlinks, unsafe filenames,
case-colliding members, and incomplete publication metadata. **It does not
authenticate the manifest or discover Wikimedia releases**; those SRC-001
requirements remain open. No active snapshot is mutated by this check.

## Structured media attribution (PACK-003 / SEC-002 partial)

The shared pack builder can now collect revision-scoped media attribution
notices for every **typed** structured media block, in deterministic index
order without duplicates. Source URL, MIME type, creator, license,
attribution and A/V preview status are retained for offline display. The
article model must validate successfully; unresolved opaque HTML fallback
fails closed. This **does not** certify an offline-ready pack: later rendered
HTML/CSS, SVG/MathML, infoboxes, thumbnails, captions, external fonts and
complete required-media verification are still missing. Display layers must
escape attribution strings from untrusted content; audio/video payloads must
not be bundled by default.

## CI and evidence

`.github/workflows/rust.yml` checks root Rust fmt, Clippy, unit tests and exact Git SHA. `.github/workflows/platform-shells.yml` builds the desktop frontend, Android debug APK/tests and checks Tauri native Rust dependencies on Ubuntu 24.04. A clean CI build does not mean the prototype shell has been installed or exercised on a device.

Do not mark canonical TODO tasks complete until their tests and platform acceptance have been recorded at the exact commit SHA.

Partial implementation and CI receipts for the current development session are in [IMPLEMENTATION_EVIDENCE_2026-10-09.md](IMPLEMENTATION_EVIDENCE_2026-10-09.md). The canonical TODO remains authoritative.
