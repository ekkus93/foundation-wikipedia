# STORE-001: snapshot catalog title identity — 2026-10-14

Status: partial implementation. STORE-001 remains unchecked.

Source head: `4878b98abd053edd6c8822239004016a6be7abb3`.
Rust workspace CI 38582488279: success.
Platform shells CI 38582488280: success.
Android Rust ABI CI 38582488382: success.
Android record codec probe 38582488282: still running when recorded.

The SQLite title index now treats underscores as spaces and uppercases only the initial Unicode scalar when its uppercase is one scalar. Remaining title case is preserved; `Gravity` and `GRAVITY` are distinct, while `gravity` resolves to `Gravity`. A regression verifies lowercase-initial lookup, rejects incorrectly folded uppercase spelling, and exercises atomic collision rejection for `Earth` and `earth` on different page IDs.

This is a fixture-backed catalog correctness fix, not a claim of a complete snapshot import, source authentication, mandatory media, physical Android qualification or offline pack acceptance.

## Shared title-key refactor

The catalog now delegates first-letter title-key generation to `wiki-model` instead of keeping an independent algorithm. Head `b7bd437c7bd12d3300fa27c701ac3c192dad5206`: Rust CI 38583652220, Platform CI 38583652425 and Android ABI CI 38583652565 passed. Android codec probe CI 38583652358 was still running when recorded. STORE-001 remains unchecked pending full snapshot and device acceptance.
