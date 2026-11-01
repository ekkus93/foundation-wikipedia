# AI-004 — Local endpoint and locality integrity at the desktop IPC boundary

**Date:** 2026-10-31  
**Implementation:** `a09653e3fe97190b8bec8faa06b52a7eaaf0e4dd`  
**Status:** Partial; AI-002 and AI-004 remain unchecked.

## Problem and scope

The native Rust local provider and saved-profile adapters already reject public or DNS destinations and pin approved local socket addresses. The frontend's saved-profile decoder previously accepted any syntactically plausible `http://` URL, including public hosts, while the inventory decoder trusted a native-reported `localhost` versus `local-network` label without independently comparing it to the requested address. A forged/malformed IPC response could therefore misrepresent a remote destination in Settings even though the native network boundary remained restrictive.

`desktop/src/aiLocalModels.ts` now uses a conservative, **display-only** classifier for exact HTTP localhost, loopback IPv4/IPv6, RFC1918 IPv4 and IPv6 unique-local destinations. It rejects public, DNS, link-local and malformed hosts, invalid ports, credentials, query/fragment/encoded paths and unsupported paths. It compares the inventory locality against the endpoint's independently derived locality, and validates saved profile endpoints with the same policy. The native Rust adapter remains the sole authority to open a pinned socket; this frontend check is not permission to connect.

## Regression and CI

- `desktop/scripts/aiLocalModels.test.mjs`: positive IPv4/IPv6/locality cases; rejected public/DNS/unsafe/ambiguous addresses; forged saved-profile and native-inventory payloads.
- Initial `4ea779a2866dadfbf84eb14a60e2ef76ff9e9267` failed TypeScript type narrowing ([desktop smoke 41276372660](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41276372660)); the exact corrected code is `a09653e3fe97190b8bec8faa06b52a7eaaf0e4dd`.
- [Rust workspace 41276539940](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41276539940) — passed.
- [Desktop launch smoke 41276540026](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41276540026) — passed, including the frontend build/test.
- [Platform shells 41276539975](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41276539975) — pending at documentation.

## Still required

Live local Ollama/llama-server LAN acceptance, explicit connection/inference consent and passage disclosure, remote-provider credential storage, grounded citation streaming, interactive cross-window conflict acceptance and physical desktop/Android qualification. The classifier intentionally rejects HTTPS until authenticated local HTTPS is implemented; IPv6 link-local and DNS aliases are not treated as private-LAN identity. No AI-002/AI-004 completion checkbox is justified.
