# Manifest reinstall collision — STORE-002 / PACK-003 review

An existing pack owner can be given a different required-media inventory by calling `install_manifest_media_batch` again with the same owner ID. The current implementation validates the new manifest against the supplied bytes, then uses additive `register_verified_owned_batch`. Old owner records remain, so the resulting owner contains the union of old and new inventories. `verify_required_owner_media` correctly detects this later, but the install helper returns success without asserting that postcondition.

Required remediation: reject conflicting owner inventories before staging; enforce the same invariant inside the SQLite registration transaction to prevent concurrent installers from racing; verify exact post-install ownership; retain idempotent reinstall of an identical inventory. Add tests for conflicting reinstall, duplicate identical reinstall, cross-pack shared media and concurrent writers.

This review is not acceptance evidence. STORE-002 and PACK-003 remain incomplete; no pack should be activated on this helper's return value alone.
