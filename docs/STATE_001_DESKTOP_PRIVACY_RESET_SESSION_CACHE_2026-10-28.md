# STATE-001 / DUI-003 — Desktop personal-data reset clears session and fences cache

Date: 2026-10-28. Scope: desktop preview shell and private user-state clearing.

## Problem

The desktop's Delete personal reading data action previously cleared private SQLite reading rows and the native in-memory online HTML cache, but left React-mounted article previews, tab labels, Back/Forward history, and scroll snapshots visible in the same running session. Additionally, an old online title/page worker queued before the reset could begin after the cache had been cleared and repopulate the cache.

## Implementation

- After a successful native `clear_reader_personal_data`, `App` clears per-tab article contexts, scroll snapshots and pending bookmark navigation, resets to one empty tab and increments a session key. The new key remounts the reader and drops previously mounted sandboxed article frames and navigation stacks. It does not clear installed packs or appearance settings.
- `DesktopOnlineCache` now carries an atomic generation alongside the shared mutex-protected `OnlineRevisionCache`. Each Tauri title/page command captures the generation before queuing its blocking worker. A privacy reset increments the generation **before** acquiring the cache lock and clears cached HTML under that lock. Old-generation workers always take the uncached online path; the generation is checked again under the lock to close the preflight/lock race.
- Offline requests remain network-free. The change does not add iframe privileges or alter publisher revision validation.

## Verification

Implementation SHA: `5f9db63906a09c0d493dcf76911a04c4c3e80012`.

- [Rust workspace CI 40685917029](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40685917029): success.
- [Platform shells CI 40685917222](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40685917222): success.
- [Desktop launch smoke CI 40685917155](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40685917155): success.
- Native fake transport verifies that a stale-generation page request still returns a preview but leaves the cache empty, and that a subsequent fresh-generation request can cache verified revision HTML. Existing title/page cache-sharing and offline isolation tests continue passing.

## Remaining qualification

This is an in-process privacy reset and stale-cache-write guard. A previously started HTTP request may still finish, though it cannot rehydrate the reset UI or refill the old cache. It does not erase external browser/OS/network traces, validate physical desktop/Android behavior, complete mutable-state migrations, or qualify verified offline pack/media installation. **STATE-001 and DUI-003 remain unchecked.**
