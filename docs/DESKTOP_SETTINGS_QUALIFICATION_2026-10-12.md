# Desktop Settings shell — qualification note (2026-10-12)

Scope: partial STATE-002. Desktop now has the nine specified Settings categories, nested navigation and back-to-article scroll restoration. Appearance & Reading offers System, Light and Dark themes. The non-secret preference currently uses browser localStorage; shared SQLite state is still pending.

Qualified code SHA: `a3f6b15e3ab8e9634db599de2366fe3da62614ff`.

- Rust workspace CI: 38343606417 — passed.
- Platform shells CI: 38343606505 — passed.
- Desktop launch smoke CI: 38343606498 — passed.

Limitations: no real Wikipedia article rendering, full browser interaction/scroll accessibility acceptance, SQLite settings, chat/window restoration or functioning other Settings pages. STATE-002, DUI-001 and UX-001 remain unchecked.
