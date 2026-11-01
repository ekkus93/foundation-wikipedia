# AI-004 — Monotonic local-profile revision closes stale-editor ABA gap

**Date:** 2026-10-31  
**Verified implementation:** `90ea05da5adc129f53a5daec7ad77f1a55bb547e`  
**Status:** Partial AI-004; parent checkbox remains open.

## Problem

The previous desktop profile precondition compared the full saved profile collection and SQLite CAS checked the exact persisted JSON. This prevents ordinary lost updates but not an **ABA** sequence: another window can change profile A to B and back to A. The original editor's collection then compares equal again, even though an intervening mutation occurred. Focus/event reconciliation could also adopt a newer CAS snapshot while retaining an older draft; a separate explicit-review fence now covers that UI path.

## Implementation

- The existing `ai.local_profiles.v2` JSON gains a `revision` field (no new storage key or destructive migration). Old v2 rows without the field and read-only v1 compatibility views default to revision 0.
- Each successful explicit upsert, activation and removal increments the revision before the existing atomic SQLite compare-and-swap. Empty collections retain their revision; deleting the final profile does **not** reset the clock.
- Native validation rejects values above JavaScript's exact integer range, and increments fail closed rather than wrap.
- `expectedProfiles` now includes the revision. A stale editor cannot replay A after an intervening A→B→A cycle because the generation differs.
- Frontend profile parsing accepts the historical revision-less shape as generation 0, validates new generations as nonnegative safe integers, and includes revision in snapshot comparison and conflict-review decisions.
- Tests cover ABA replay, stale mutation rejection, revision-less v2 migration, deletion preserving generation, invalid frontend revision values and generation-only change detection.

## Exact-head verification

Initial code `6deeda84830c49df35dd906f8f680d9b343cca25` failed a frontend test fixture that lacked revision 0 ([desktop smoke 41278893446](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41278893446)). Subsequent fixes `198ab30ef7cd74f3e454ca5b044e818b913c8ae0` and `5d8554b1ecbb818d00cd8095a39f983bd0e26b58` corrected outdated native assertions treating an empty collection as generation zero ([41278969825](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41278969825), [41279138663](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41279138663)).

All workflows passed for the corrected implementation SHA `90ea05da5adc129f53a5daec7ad77f1a55bb547e`:

- [Rust workspace 41279356291](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41279356291).
- [Platform shells 41279356273](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41279356273).
- [Desktop launch smoke 41279356270](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41279356270).

## Remaining acceptance

This change closes ABA for writes using the new versioned native protocol; it does not qualify cross-version running processes that still write legacy revision-less JSON. The project still needs interactive multi-window desktop acceptance, real Ollama/llama-server LAN tests, cloud credential storage and consent, article-grounded streaming citations, offline media and physical Android/desktop qualification. AI-002 and AI-004 remain unchecked.
