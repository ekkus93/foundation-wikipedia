# AI-004 — Notify desktop windows after local-profile changes

**Date:** 2026-10-31  
**Exact implementation SHA:** `7542a560cb6ae14e3c71118e9c4beb0c06ebd6fb`  
**Status:** Partial AI-004; canonical parent remains unchecked.

## Behavior

- Native `upsert_local_ai_profile`, `activate_local_ai_profile`, and `remove_local_ai_profile` emit `ai-local-profiles-changed` **only after** a successful persisted compare-and-swap. The event payload is empty; profile names, endpoints, model identifiers and credentials are never broadcast.
- Each AI Settings window listens for the event and re-reads the private SQLite store through `read_local_ai_profiles`. It validates the native response and updates the saved-profile list only if the collection differs. An unsaved profile name, endpoint, protocol, model choice and discovered inventory are preserved.
- Existing focus refresh now also runs when the document becomes visible. These fallback reads cover events missed while settings is closed, listener-registration failures and **other desktop processes**, which do not share Tauri's in-process event bus.
- Request sequencing, edit-revision checks, cleanup on unmount, suspended refresh during local mutations, native expected-collection validation and SQLite CAS remain in place. The event itself is only a hint to reread, not an authorization to connect to a model or mutate saved settings.

## Automated evidence

The exact code SHA `7542a560cb6ae14e3c71118e9c4beb0c06ebd6fb` passed:

- [Rust workspace — 41274447445](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41274447445) (format, strict Clippy, unit tests).
- [Platform shells — 41274447498](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41274447498) (frontend TypeScript checks/tests and native shell).
- [Desktop launch smoke — 41274447470](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41274447470).

These CI checks establish compilation and existing regression coverage, **not** an interactive multi-window UI test. The native event path still needs a multi-window acceptance test, including a missed event, concurrent saves, focus/visibility fallback and the absence of outbound network traffic.

## Remaining work

This event is best-effort and limited to one Tauri process; independent desktop processes synchronize only on focus/visibility. No durable revision token/ABA fence, interactive conflict resolution, remote-provider consent/credentials, real inference, Android parity or physical-device acceptance is implemented by this slice. Keep AI-004 unchecked.
