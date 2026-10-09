# Foundation Wikipedia: partial implementation evidence

The canonical checklist is docs/WIKIPEDIA_AI_READER_TODO.md. All related parent tasks remain incomplete.

- BOOT-003: fixture provenance host checks and empty-suite rejection at 7145522acc252aeec1f08b835683f5a08b66f557. CI 37930979946 and 37930980089 passed.
- SRC-001: safe source filenames and generation IDs at ea70336fe20930d76acad3078a8346e47398819a. CI 37931757948 and 37931757870 passed.
- MOD-001: media metadata checks and cross-project redirect validation, last updated at eeade0f4ba26a7a35c88fe5f1ede83a06d7ca764.
- MOD-002: revision-scoped reference handles and enumeration at 4825481136f2c4d06a1463e3cee606b05fdfb598. CI 37931418794 and 37931418832 passed.

Next: continue unchecked BOOT-002/BOOT-003 and MOD-001, then dependent tasks. Do not mark completion without full acceptance evidence.

- PACK-002: category resolver regression coverage for cycles, deterministic order, depth/page bounds, invalid input and exclusion precedence; latest test commit `bd48228bea17aeaed14f1ef681fd9a4968755856`. Rust CI `37960766495` passed at that exact SHA. The resolver still lacks official category integration and snapshot/definition provenance; PACK-002 remains unchecked.
- PACK-001: portable manifest validation rejects path-like pack IDs, invalid project/snapshot identifiers, Windows-reserved object components, trailing-dot/space components and ASCII case-colliding object names. Code at `5388d7aeade12ac7177ce76642b4000897ef1274`; Rust CI `37961256845` passed at that exact SHA. Full container framing, signatures, streaming install and Android qualification remain unchecked.
- BOOT-003: added a fail-closed root Rust dependency SPDX license allowlist audit and negative regression tests in `scripts/check_dependency_licenses.py` and `scripts/test_dependency_licenses.py`. The existing Python unittest discovery gate runs the audit against actual `cargo metadata --locked` packages. Rust CI `37961761204` passed at exact SHA `9500a38efadf8bd869bd3309de3eec86fe7ebab7`. This does not yet audit desktop npm, Tauri's separate workspace, or Android Gradle dependencies; BOOT-003 remains unchecked.
- PACK-002: a finite category-visit budget now fails closed with `TooManyCategories` for graph fanout that would otherwise traverse arbitrarily many empty categories without exceeding the article page limit. Synthetic fanout regression test at `8944d046d77fb1fbc18c440794644ece7322d9bb`; exact-head Rust CI `37962371516` passed. Official category graph ingestion, redirects and fixed-snapshot manifests remain outstanding, so PACK-002 remains unchecked.
