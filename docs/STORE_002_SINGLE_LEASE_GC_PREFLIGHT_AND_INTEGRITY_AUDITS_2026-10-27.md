# STORE-002: whole-pass shared leases for audits and advisory GC

Date: 2026-10-27  
Status: **partial qualification; STORE-002 remains unchecked**

## Scope and rationale

The registered-media integrity audit previously obtained a SQLite registry/owner/notice snapshot and then acquired and released an independent media lease per digest. A cooperating exclusive collector could remove a digest between the snapshot and its later file verification. The two read-only GC planners had similar gaps between enumerating SQLite/filesystem candidates and verifying the on-disk content. These races could cause misleading or spurious audit failures; they are not safe deletion authority.

The registered integrity audit now acquires one SHARED media lease **before** the first registry query, retains it through attribution and owner checks and every streaming SHA-256/size verification, and calls the already-lease-held `verify_object_size_unlocked` helper to avoid nested SQLite lease acquisition. `plan_unowned_media_gc` holds a SHARED lease from SQLite candidate enumeration through verification. `plan_unregistered_media_gc` holds the same lease from canonical filesystem enumeration through registry checks and streaming verification. The existing reserved planner and collectors continue to acquire EXCLUSIVE leases before SQLite IMMEDIATE writer reservations. No public read-only plan permits unlinking.

## Regression and CI evidence

A new persistent SQLite test holds an EXCLUSIVE lease and launches a concurrent full registered-media audit. It verifies the audit cannot complete until release and then returns the expected two-object byte count. A second persistent SQLite test holds an EXCLUSIVE lease and launches both advisory planners in separate threads. Neither returns before lease release; afterward each reports exactly its expected registered-unowned or unregistered-orphan digest.

- Audit implementation SHA `3b1d339e8a6562f7d190082b86777bcdf1d9ebb5`: [Rust workspace 40512905044](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40512905044), [Platform shells 40512905071](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40512905071), [Android Rust ABI 40512905084](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40512905084) **passed**.
- Final planner/regression SHA `c2cc93b48ba726c0e8fa9ab1267964123262dd21`: [Rust workspace 40513334958](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40513334958), [Platform shells 40513334938](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40513334938), [Android Rust ABI 40513335004](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40513335004) **passed**; [Android record codec probe 40513334895](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40513334895) still running when recorded.

## Limitations and follow-up

These leases protect against cooperating media readers, publishers and collectors. Direct SQLite writers can still mutate owner rows; the audit is **not** an immutable registry snapshot. A hostile writer able to replace directories or mutate files outside the lease protocol remains outside this protection. Physical power-loss/disk-full fault injection, real pack/uninstall workflow integration, scheduled maintenance, publisher-authenticated source/attribution, required-visual completeness and desktop/Android device acceptance remain open.

**Do not mark STORE-002 complete.**
