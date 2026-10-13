# SRC-002 — Joined page/revision integer bounds (open review)

**Status:** Identified, not implemented or qualified. Observed on master `4c54d2dfe22b2d9e6ddf91ad63a56cebd46f83d6`.

The `wiki-source::join::collect` input guard rejects zero page and revision IDs, but does not reject `u64` values greater than `i64::MAX`. The two deletion-marker join paths similarly reject zero page IDs without enforcing the signed 64-bit limit. Downstream SQLite-backed catalogs and caches require positive signed 64-bit IDs. Rejecting out-of-range IDs at source ingestion would avoid accepting an unusable canonical identity.

**Suggested implementation:** Add `> i64::MAX as u64` rejection for component page IDs, component revision IDs, and both deletion-marker page-ID validation paths. Add synthetic regressions that set IDs to `i64::MAX as u64 + 1` and `u64::MAX` without using the fixture constructor's page-ID arithmetic (which can overflow). Confirm exact-head Rust CI and all required platform checks. Do not mark SRC-002 complete from this guard alone.

**Attempt history:** Ralph Bridge read and repository state operations worked, but attempted source edits to `crates/wiki-source/src/join.rs` were rejected by the tool-level safety gate. No join code changes were committed. This report is a handoff for a later authorized write attempt; the failure is not evidence that the change was applied.
