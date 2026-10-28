# PACK-003 — Rendered-object audit fixture race and CI recovery

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Source commit:** `e47f9318dce560e29de80437a1c95f53747a8066`

## Failure and root cause

The documentation-only master commit `fb111d1612440c6c75dc5f15b18873d5ce514883` failed [Rust workspace CI 40624270759](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40624270759) at the test `rendered_objects::tests::css_discovery_bytes_must_match_verified_local_stylesheet`. The test fixture used only process ID and nanosecond wall-clock time to derive a temporary directory name, then unconditionally `create_dir(...).unwrap()`. Parallel workers can choose the same timestamp, causing a nondeterministic `AlreadyExists` panic unrelated to application logic.

## Implementation and qualification

- `crates/wiki-pack-builder/src/rendered_objects.rs`: fixture root names additionally contain an atomic process-local sequence number; directory creation retries `AlreadyExists` and reports other filesystem errors.
- New `parallel_fixtures_have_independent_media_roots` starts 32 concurrent workers, verifies that the content-addressed media can be read from each directory, checks pairwise distinct root paths, and cleans up each fixture.
- Corrected exact source SHA `e47f9318dce560e29de80437a1c95f53747a8066`: [Rust workspace CI 40624583504](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40624583504) **passed**; [Platform shells CI 40624583444](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40624583444) **passed**.

This repairs test isolation and stops CI from reporting false negatives due to collisions. It does not authenticate Wikimedia sources or media provenance, prove complete offline visuals, perform platform device acceptance, or qualify PACK-003 as done.
