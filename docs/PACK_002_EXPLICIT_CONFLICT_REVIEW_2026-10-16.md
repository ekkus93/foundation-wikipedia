# PACK-002: explicit include/exclude collision review

The deterministic resolver in `crates/wiki-pack-builder/src/selection.rs` applies exclusions to canonical redirect targets before explicit includes. This is correct for the selected page set but does not explain to the user why a requested explicit include was dropped.

Required follow-up: add a deterministic pre-download warning for every explicit include suppressed by an exclusion after redirect resolution. Preserve the selected page IDs and canonical selection fingerprints. Test both direct and redirect-mediated collisions and root/include order independence.

Qualification remains pending. PACK-002 stays unchecked.
