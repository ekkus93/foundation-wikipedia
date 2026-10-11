# Manifest reinstall collision — STORE-002 / PACK-003 review

An existing pack owner can be given a different required-media inventory by calling `install_manifest_media_batch` again with the same owner ID. The current implementation validates the new manifest against the supplied bytes, then uses additive `register_verified_owned_batch`. Old owner records remain, so the resulting owner contains the union of old and new inventories. `verify_required_owner_media` correctly detects this later, but the install helper returns success without asserting that postcondition.

Required remediation: reject conflicting owner inventories before staging; enforce the same invariant inside the SQLite registration transaction to prevent concurrent installers from racing; verify exact post-install ownership; retain idempotent reinstall of an identical inventory. Add tests for conflicting reinstall, duplicate identical reinstall, cross-pack shared media and concurrent writers.

This review is not acceptance evidence. STORE-002 and PACK-003 remain incomplete; no pack should be activated on this helper's return value alone.

## Implemented fix (partial roadmap qualification)

The conflicting owner-reinstall defect was corrected in exact code commit `f0907f1eb99c77ca68c449bc3faca5cc4245823f`. The SQLite batch registry uses an IMMEDIATE transaction to compare the existing owner inventory against the complete incoming digest/size set, rejecting changes rather than accumulating old and new media. The manifest installer performs early conflict preflight and exact post-install object/owner verification. Tests exercise idempotent reinstall, conflict rejection before staging, separate-pack ownership and concurrent competing installs.

Exact-head CI passed: Rust workspace run 38105768694; Platform shells run 38105768712; Android Rust ABI run 38105768748. Android codec emulator probe run 38105768726 was still in progress when this note was prepared. This is not full STORE-002/PACK-003 acceptance: authenticated pack activation, crash-safe GC, complete renderer media discovery and real-device airplane-mode checks remain outstanding.

Additional direct registry rollback regression at `547e921cfadf04ecbced9c40d2d6f2aa375094a1`: Rust workspace CI 38106151630, Platform shells CI 38106151726, Android ABI CI 38106151689 passed; Android codec probe 38106151650 pending. Conflicting owner inventories are rejected before any new notice/owner writes; identical inventories may add distinct valid attribution.
