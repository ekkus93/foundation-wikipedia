# AI-005 — Desktop saved chat thread-list identity fence

**Date:** 2026-11-01  
**Repository:** `ekkus93/foundation-wikipedia`  
**Qualified source SHA:** `82beefe004258727392a6cd85501a2083eb23341`  
**Status:** Partial; AI-005 remains unchecked.

## Problem

The saved-chat panel's initial `list_reader_chat_threads` IPC response previously used only an effect-local `active` boolean. During navigation, React can render a different article before the previous effect's cleanup invalidates that boolean. A late response could then briefly select the previous article's thread, show its list error or alter the loading state. The desktop currently keys its chat panel by article identity, but a request-level identity guard makes the component safe independently of its parent's mount policy.

## Implementation

Added `ReaderChatThreadListFence` to `desktop/src/readerChatHistory.ts`, independent from the existing delete and exchange-read fences. Each initial list request records an exact project/page/revision and a monotonically increasing generation. The panel compares responses against the current article ref updated during render, before applying thread IDs, errors or loading-finalization state. Effect cleanup invalidates the token. Returning to an article with identical identity still requires a new generation, so an older request cannot be reused.

The new fence is applied only to the read-only saved-conversation list. No article text, model prompt or network request is added.

## Verification

Extended `desktop/scripts/readerChatHistory.test.mjs` to cover changed project, page and revision, explicit invalidation, same-identity remount/re-request and obsolete completion rejection.

Exact source SHA `82beefe004258727392a6cd85501a2083eb23341` passed:

- [Rust workspace — 41364052285](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41364052285)
- [Platform shells — 41364052299](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41364052299)
- [Desktop launch smoke — 41364052359](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41364052359)

## Remaining

Desktop chat remains read-only; canonical article loading, exact evidence/model disclosure, explicit consent, cancellable generation, validated-answer persistence, clickable citations and Android/live-device acceptance are still outstanding. This regression is a frontend state-isolation test, not a physical UI race test.
