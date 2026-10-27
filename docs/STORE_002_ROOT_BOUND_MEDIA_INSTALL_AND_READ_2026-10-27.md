# STORE-002 — Root-bound media installation, reading and verification

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Scope and implementation

Added opt-in root-bound production-facing media APIs in `crates/wiki-store/src/media_install.rs`:

- `install_bound_owned_media` and `install_bound_owned_media_batch` reject a registry without a persistent media-root binding or one bound to another canonical root. The binding check occurs **after acquiring one SHARED media lease and before writing any object bytes**; that lease is retained through content-addressed publication and SQLite owner registration. The batch retains its existing all-owner-claims atomicity; interrupted staging may still leave recoverable unregistered bytes.
- `read_verified_bound_owned_media` checks binding inside the same SHARED lease spanning owner lookup, attribution validation and SHA-256-verified object read.
- `verify_bound_registered_owner_bytes` and `verify_bound_required_owner_media` likewise check binding inside the SHARED lease covering the owner inventory and every streamed object verification. The latter compares the complete declared required-media list with the registered owner's inventory, but the caller must authenticate that declaration separately.

Existing unbound methods remain available for controlled test fixtures and migration. They are **not suitable for production code that relies on the bound registry / garbage-collector safety contract**. The new APIs deliberately reuse existing installer and verifier implementations rather than taking nested SQLite-backed leases, which could deadlock when an exclusive collector is waiting.

## Verification

A disk-backed SQLite regression exercises an unbound registry, a deliberately mismatched second media root, and a properly bound root. Both single and batch bound installs fail before any byte publication or ownership mutation for unbound/mismatched registries. The accepted batch installs two unique digests, a user pin reuses one digest without duplicating storage, and the owner claims and root binding survive a separate SQLite reopen. Bound reads return the exact bytes and attribution notices, reject a wrong root, and both bound inventory verifiers check the correct total byte count; wrong-root required-media verification is rejected.

Initial installer source `878d74b7f8f1f92e366a37d7492b0ad4b5c2dfd0` failed formatting ([Rust 40579918287](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40579918287)). Corrected source `fde4488de59f7d04ab7de7217f38645ea97f0db0` passed [Rust workspace 40580046981](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580046981), [Platform shells 40580047031](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580047031) and [Android Rust ABI 40580047076](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580047076). Final bound reader/verification source `f7da3dfcec702daefc655ba578e547b28d9b6243` passed [Rust workspace 40580453944](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580453944), [Platform shells 40580453853](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580453853), and [Android Rust ABI 40580453896](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580453896). [Android record codec probe 40580453843](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40580453843) remained in progress when documented.

## Limitations and next work

These APIs do not migrate an unbound legacy registry, automatically wire pack/catalog/snapshot activation or removal, verify an official publisher signature, or guarantee that every offline article visual was declared. The binding is an accidental-cross-root guard, not a cryptographic trust root; direct same-user SQLite mutation, malicious filesystem replacement and physical disk-full/power-loss behavior remain unqualified. Production call sites must be audited and switched to bound entry points, and real-device acceptance remains outstanding.

**Do not check STORE-002 complete.**
