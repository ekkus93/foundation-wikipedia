# PACK-002: explicit include/exclude collision review

The deterministic resolver in `crates/wiki-pack-builder/src/selection.rs` applies exclusions to canonical redirect targets before explicit includes. This is correct for the selected page set but does not explain to the user why a requested explicit include was dropped.

Required follow-up: add a deterministic pre-download warning for every explicit include suppressed by an exclusion after redirect resolution. Preserve the selected page IDs and canonical selection fingerprints. Test both direct and redirect-mediated collisions and root/include order independence.

Qualification remains pending. PACK-002 stays unchecked.

## Regression qualification

`tools/wiki-pack/tests/topic_include_exclude_precedence.rs` now asserts canonical exclusion precedence for both direct IDs and redirected includes, deterministic selection/definition hashes under reordered inputs, and no publisher/offline trust elevation. Ralph operation `456fba1d321e4a1d8d858a41b5dcbf77` committed it on `master` at `7ea51d8e8b793fb115e35502e2eedb3ae9b5862b`. Exact-head Rust CI 38949275259 and Platform CI 38949275298 both passed. The separate human-readable suppression warning remains unimplemented; PACK-002 is still unchecked.
