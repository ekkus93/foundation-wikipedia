# SRC-003 — Multi-member resumable generation recovery (2026-10-25)

Status: synthetic end-to-end transport regression only; SRC-003 remains unchecked.

The host CLI's generation-level pinned inventory orchestration is now
covered by an integrated two-shard regression using the production
`stage_resumed_member_with` path. A first run finishes shard one and
stops after shard two's first synced range and a synthetic network failure.
A second invocation rehashes and reuses shard one without a request,
resumes shard two at its exact persisted offset, and atomically publishes
only after the full pinned SHA-256 passes. A third invocation reuses both
shards without HTTP. The test verifies deterministic page-range order,
byte-exact published outputs, no prematurely published second shard and
no orphaned partial files after success.

This is synthetic range transport and filesystem qualification, not a
live Wikimedia download or independent publisher-authenticated generation
completeness. Revision-matched HTML, mandatory media, offline pack
acceptance and real-device checks remain outstanding.
