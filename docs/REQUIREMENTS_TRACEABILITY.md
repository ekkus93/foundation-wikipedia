# Requirement-to-TODO traceability (v0.1)

The [SPEC](WIKIPEDIA_AI_READER_SPEC.md) defines normative behavior; the
[canonical TODO](WIKIPEDIA_AI_READER_TODO.md) is the sole completion record.
This mapping is **not** completion evidence.

| SPEC requirement | Implementation TODO IDs | Qualification |
| --- | --- | --- |
| §2 shared core and desktop/Android shells | BOOT-001–BOOT-003, MOB-001 | Cross-platform builds, real launch, ABI/device bridge |
| §4 authoritative source and revision joins | SRC-001–SRC-005 | Verified completed snapshots and exact revision joins |
| §5 canonical blocks and evidence IDs | MOD-001–MOD-003, RAG-002 | Unicode/serialization, stale citation rejection |
| §6 snapshots and mutable state | STORE-001–STORE-004, STATE-001 | Random read, crash recovery, migrations |
| §7–§10 pack, trust, mandatory media | PACK-001–PACK-006, TRUST-001–TRUST-003 | Hash/signature/tamper, visuals offline |
| §11–§12 desktop reader/action hub | DUI-001–DUI-003, UX-001–UX-004 | Navigation, selection, accessibility |
| §13–§15 retrieval and AI | RAG-001–RAG-004, AI-001–AI-005 | Grounding, provider isolation, no cloud fallback |
| §16–§17 desktop pack builder | PUI-001–PUI-005 | Definition roundtrip, CLI parity, recoverable jobs |
| §18 Android reader and pack manager | MOB-001–MOB-004, MOBPACK-001–MOBPACK-003 | Real-device airplane mode and verified import |
| §19 updates/catalog | UPD-001–UPD-004 | Publisher verification, rollback and custom rebuild |
| §20 settings, bookmarks, chat | STATE-001–STATE-003, UX-003 | Restart persistence and return-state |
| §21 security, attribution and privacy | SEC-001–SEC-003 | Hostile inputs, attribution, network/secret audit |
| §22 release qualification | QA-001–QA-003, REL-001–REL-003 | Exact-head desktop/Android E2E artifacts |
| §23 evaluations | MOD-003, SRC-002–SRC-003, LOCAL-001 | Measured ADRs, not assumptions |

When reconciling any checked task, record exact master SHA, commands, CI run
IDs/URLs, devices and evidence in the TODO. New MUST requirements require
a corresponding TODO entry; no deferral without a SPEC change.
