# STORE-002 — Root-bound manifest media installation

Date: 2026-10-27  
Status: **partial qualification — STORE-002 remains unchecked**

## Scope and rationale

The existing `media_manifest_install::install_manifest_media_batch` checks exact caller-provided SHA-256/byte inventories and attribution, then stages and registers owner claims. However, it still called the **unbound** batch install and verification APIs after production root-bound media primitives were introduced. A caller could therefore accidentally pair a valid registry with the wrong media root.

Added `install_bound_manifest_media_batch(store, registry, assets, required, kind, owner_id)`. It preserves all manifest validation and owner-inventory conflict checks, but dispatches to `install_bound_owned_media_batch` and `verify_bound_required_owner_media`. The bound batch holds one SHARED media lease from registry-root identity validation through publication and SQLite owner commit; the subsequent required-media verification obtains its own SHARED lease and checks the same binding before reading the owner inventory and hashing every object. This is **not** one continuous lease spanning both stages; no snapshot activation is performed, and the code does not establish publisher trust. The existing unbound entry point remains for explicit migration/fixtures.

## Regression evidence

A disk-backed SQLite test creates two independent media directories and an initially unbound registry outside both object trees. It verifies that unbound installation fails with `UnboundRoot` without publishing any digest or adding owner claims. After explicit binding to the intended root, installation against the other directory fails with `RootMismatch` and still publishes no digest. Installation into the correct directory succeeds, preserves distinct attribution notices on deduplicated content, and retains a separate user pin. The test closes and reopens the registry, re-verifies the exact required inventory, repeats an idempotent installation, and confirms a conflicting manifest is rejected before staging new content.

Initial source `146f455d0aa8a0828d6f3c799daa4b3018f0733d` failed formatting ([Rust workspace 40587567760](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40587567760)). Corrected source **`c0af8da1ca2655f7bb130698f3f876a6d4361593`** passed [Rust workspace 40587649401](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40587649401), [Platform shells 40587649383](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40587649383), and [Android Rust ABI 40587649456](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40587649456). [Android record codec 40587649468](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40587649468) remained running when documented.

## Explicit non-goals and remaining work

- The required-media manifest is supplied by the caller. This API does **not** validate a publisher signature, check article-to-media completeness, or confer official trust.
- The ownership transaction is not a complete pack install. Snapshot/catalog activation, rollback, persisted pack metadata and desktop/Android lifecycle wiring remain outstanding.
- The root-binding check prevents accidental wrong-root use by cooperating callers; it is not a defense against same-user hostile direct SQLite writes, filesystem replacement, or physical power-loss faults.
- Real pack/device acceptance and fault injection are still required. **Do not check STORE-002 complete.**
