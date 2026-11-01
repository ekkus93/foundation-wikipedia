# UX-002 — Movable desktop chat history window

**Date:** 2026-11-01  
**Scope:** Partial UX-002 / AI-005. No parent task completion.  
**Implementation commits:** `f30fc9a6b46d66fa9d86692ddadc8475badd50bc`, `8fbfd51a5d79181d4f1868348aecf543b8ae4440`

## Implemented

The desktop's existing exact-article-revision, read-only saved-chat panel now behaves as a nonmodal floating window. Users can drag its title handle with a pointer or move it with arrow keys (Shift+Arrow moves 48 pixels, otherwise 16 pixels), resize the panel using the native CSS resize affordance, and minimize/restore it without deleting or mixing saved conversation content. The close button restores keyboard focus to the reader action hub.

`readerChatWindow.ts` clamps movement to the actual measured panel size and viewport, including after a viewport resize or CSS panel resize. A small margin is maintained when space permits. `ResizeObserver` and a viewport resize listener keep the panel reachable. Hidden panels skip zero-size geometry updates.

The chat component remains mounted while navigating temporarily to Settings. Its minimized state, user-selected position and native resized dimensions survive that round-trip. Switching article tabs or revisions still remounts the exact-revision conversation panel rather than reusing the previous article's messages. Personal reading-data deletion still closes the panel.

The panel remains explicitly **read-only**: previously generated text is not treated as validated citations, and online text previews are not promoted to canonical RAG evidence.

## Verification

The frontend test suite now includes `desktop/scripts/readerChatWindow.test.mjs`, covering normal motion, all four edges, viewport shrink, oversized geometry and nonfinite input rejection. The desktop frontend typecheck/build and native launch smoke also passed for the second implementation SHA.

- [Rust workspace — 41323314299](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41323314299) — passed.
- [Platform shells — 41323314273](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41323314273) — passed, including desktop frontend.
- [Desktop launch smoke — 41323314249](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41323314249) — passed.
- The initial UI implementation also passed [Rust 41323084356](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41323084356), [Platform 41323084325](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41323084325), and [desktop smoke 41323084329](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41323084329).

## Still outstanding

Automated geometry and launch tests are **not** interactive desktop acceptance. Manual pointer/keyboard dragging, native resizing and Settings return should be verified on a real desktop with small and large viewports. Article-grounded inference, explicit passage consent, streaming/cancellation, verified clickable citations, provider disclosure in chat, Android chat and real local-model acceptance are still required. **UX-002, AI-002, AI-004 and AI-005 remain unchecked.**
