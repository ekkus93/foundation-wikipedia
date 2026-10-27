# STORE-002 — Media GC process-exit recovery qualification

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## What this tests

This regression uses a **real child process**, not only a mock callback error. The parent creates three registered-but-unowned immutable media objects, one separately registered and user-pinned object, and a persistent SQLite registry outside the content-addressed media root. It launches the current Rust test executable with an exact test filter and an isolated fixture path.

The child opens the persistent registry, acquires the EXCLUSIVE SQLite-backed media lease through the registered-media collector, and lets the collector **commit the unowned registry deletion before unlink**. The injected unlink callback deletes the first file and fsyncs its digest-prefix directory using the production implementation, then invokes `std::process::exit(77)` immediately. This intentionally bypasses Rust destructors and the collector's normal return path. The OS releases the child's SQLite locks on process termination.

The parent requires exit status 77, reopens the SQLite registry, verifies the three unowned registrations are absent, confirms the pinned object remains registered, and identifies exactly two surviving unregistered media files. The production orphan collector removes those files and reports the same exact byte total as the pre-recovery orphan inventory. A repeat orphan sweep removes zero objects, and the pinned bytes remain SHA-256 readable.

## Exact-head qualification

Initial test SHA `772179d05c1dfdf7b21ff32a7c9b9296d280b8bc` failed only rustfmt ([Rust CI 40515230077](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40515230077)). Corrected exact source SHA **`69c56f871f097ebad8116f3408d6596efb0d424b`** passed [Rust workspace CI 40515478341](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40515478341), [Platform shells CI 40515478272](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40515478272), and [Android Rust ABI CI 40515478300](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40515478300). [Android record codec probe 40515478390](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40515478390) was still in progress when recorded.

## Limitations and next work

This test is a deliberate child-process `exit` killpoint after SQLite commit and one durable unlink. It does not remove physical power, inject disk-full/write errors, simulate torn sectors, or send an external SIGKILL while the process is at an unpredictable instruction. Other commit/unlink/fsync killpoints remain worth qualifying. It does not establish security against non-cooperating direct filesystem or SQLite writers, and it does not wire the maintenance API into real pack/cache/pin removal, a scheduler, or desktop/Android UI. Publisher-authenticated media and end-to-end required-visual completeness remain open.

**Do not mark STORE-002 complete.**
