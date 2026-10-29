# RAG-002 / STORE-001 — Offline saved-citation revision bounds (2026-10-28)

All four exact-revision local navigation methods now reject revision ID 0 and revisions beyond the SQLite-safe signed 64-bit range before accessing the installed catalog. Canonical articles already require a positive, bounded revision; this aligns caller-provided saved citation identities with the record contract. A missing page/title with an invalid expected revision must be an error, not `None`, and an existing page with a malformed revision must not be reported merely as stale.

Synthetic on-disk reader tests exercise invalid revisions on missing source keys and missing titles across page, redirect-target, title and title+page exact navigation. Existing positive, stale-revision, redirect-target and digest-corruption regressions remain.

This is local identity hardening, not a publisher-authenticated snapshot, an installed pack with complete mandatory offline media, a production UI or real-device acceptance. **RAG-002 and STORE-001 remain unchecked.** Record exact source CI in the canonical TODO before claiming qualification.
