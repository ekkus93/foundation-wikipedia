# SRC-004 — Temporary revision HTML cache v2 (partial)

Implementation head: `b855cd78affc2ad612015d7597a6266a5368f8d9`

The temporary SQLite HTML cache now checks a second SHA-256 digest binding the project, page ID, revision ID, title, HTML, and fetch timestamp. It rejects corrupted rows on reads and writes. Identical revision refreshes preserve the newer fetch time and update the record digest. On schema migration from v1, only disposable temporary cache rows are discarded because their title and identity metadata was not protected. A bounded page-scoped revision enumeration validates every returned row. Synthetic tests cover title, timestamp, page, project and revision tampering; migration; enumeration; and fail-closed corruption.

## Qualification

- Rust workspace: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38448626656 — passed.
- Platform shells: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38448626691 — passed.
- Android Rust ABI: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38448626403 — passed.
- Android record codec emulator: https://github.com/ekkus93/foundation-wikipedia/actions/runs/38448626558 — running at time of writing, not cache acceptance evidence.

An unkeyed local digest is corruption detection, **not** proof of Wikimedia authenticity or resistance to an attacker who can modify the digest. HTML storage is not proof that required offline visuals, references, math, captions, and attribution are complete. Production HTTPS transport, source/media verification, reader integration and real-device article acceptance remain outstanding. **SRC-004 remains unchecked.**
