# SRC-004 / SEC-001 — cached online title Unicode integrity (2026-10-24)

## Scope

The temporary, revision-keyed HTML cache previously rejected control characters and selected bidi overrides but accepted invisible formatting characters and Unicode line/paragraph separators in its displayed title. The cache now rejects U+200B, U+2028–U+2029, U+2060, U+206A–U+206F, and U+FEFF, in addition to the existing directional-format guard. Validation applies both before insertion and when reading persisted cache entries.

Regression coverage exercises each rejected marker on write, the newly rejected markers on read even if stored SQLite data is modified, and legitimate Persian ZWNJ and Indic ZWJ titles. The latter remain accepted because those joiners are linguistically meaningful.

## Boundaries and qualification

This is a scoped display-identity integrity improvement for `SRC-004` and `SEC-001`; it does not authenticate Wikimedia responses, sanitize HTML, validate all upstream title-normalization rules, guarantee cache freshness, or qualify complete offline packs. Both parent tasks remain unchecked. CI and exact source SHA should be recorded after verification; no release/device acceptance is claimed.
