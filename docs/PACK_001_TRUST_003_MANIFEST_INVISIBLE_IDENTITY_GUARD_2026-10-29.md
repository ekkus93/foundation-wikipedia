# PACK-001 / TRUST-003 — Invisible manifest identity guard (scoped)

**Date:** 2026-10-29  
**Corrected exact source SHA:** `c9b31918ac42a4d105454c3f8b6ff1565b09d722`  
**Scope:** Provisional `.wpack` manifest validation and canonical identity transcript. This does not qualify complete pack import.

## Change

Previously `safe_id` and object path checks rejected directional controls but accepted certain invisible format characters and Unicode noncharacters. An attacker could present visually indistinguishable pack names, publisher labels, snapshot IDs, custom definition IDs or object paths with distinct identities. The new shared `unsafe_label_character` filter rejects U+200B, U+2060, U+FEFF, U+2028–U+202E, U+2066–U+206F, U+061C, U+FDD0–U+FDEF and the U+FFFE/U+FFFF noncharacters in every Unicode plane. ZWNJ/ZWJ remain accepted for legitimate multilingual labels.

The existing canonical identity transcript and byte-budget/path-traversal policies are unchanged for valid inputs. This is a lexical spoofing guard; it does not authenticate any publisher, validate media attribution or establish that a pack is installable.

## Synthetic evidence

The new manifest regression injects each prohibited scalar into pack ID, snapshot, official publisher, custom definition ID and an object path. It checks that invalid metadata/paths fail before transcript generation. A valid Persian/Indic ZWNJ/ZWJ case remains accepted.

- Initial code SHA `7321a94f1bed55a3f70640bfa7a2bce53320247a` failed the Rust formatting gate only: [Rust 40838787778](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40838787778).
- Corrected SHA `c9b31918ac42a4d105454c3f8b6ff1565b09d722`: [Rust workspace 40838868794](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40838868794) **success** (format, strict Clippy, workspace tests), [Android Rust ABI 40838868934](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40838868934) **success**, [Platform shells 40838868920](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40838868920) **in progress at documentation**. Recheck the exact SHA for terminal platform CI.

## Remaining acceptance

Complete signed publisher provenance, required offline visual/media and attribution integrity, production archive install/activation, UI trust labels and real desktop/Android acceptance remain open. **PACK-001 and TRUST-003 remain unchecked.**
