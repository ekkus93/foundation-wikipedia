# STORE-002 — MIME-only reader attribution integrity (2026-10-26)

**Status:** Scoped reader metadata hardening; STORE-002 remains unchecked.

Previously `MediaRegistry::owned_digest_mime` queried only distinct MIME values. A registry with a valid MIME but a malformed creator, license, or source URL could still return that MIME to a renderer even though the owner-scoped attribution API would reject the same object's notices. The MIME-only path could also bypass the 1,024-notice fanout limit.

Now `owned_digest_mime` derives the MIME from `owned_digest_notices`, which validates the complete bounded attribution set, requires at least one notice for an owned digest, rejects conflicting MIME declarations, and preserves pack/cache/user-pin isolation. Absent ownership still returns `None`. This is a read-side fail-closed consistency improvement; it does not prove actual media bytes or publisher authenticity.

**Tests:** Existing synthetic SQLite tests now assert MIME-only reads reject more than 1,024 notices and corrupted creator text (in addition to invalid MIME). They verify the attribution API rejects the same corruption and that a different pack cannot read the object's MIME. The existing positive and conflicting-MIME tests remain.

**CI qualification:** Source SHA **`fbfe0e0275dd4248e67c70026fa77bd4cbe3bfd2`** passed [Rust workspace 40428228703](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40428228703) and [Android Rust ABI 40428228629](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40428228629). [Platform shells 40428228655](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40428228655) and Android record codec probe 40428228677 were still running at documentation.

**Limitations:** Attribution metadata is not authenticated by this API. The code does not prove all revision-required media are present, implement concurrency-safe garbage collection or crash recovery, or qualify pack activation or physical-device rendering.
