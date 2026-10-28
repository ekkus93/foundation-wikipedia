# SRC-002 — Typed snapshot-wide Enterprise link index identity validation

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Verified source SHA:** `83640747c40dc916ea58cb6af82afe04093d42f4`

## Gap and implementation

The source-wide `EnterpriseLinkIndex::build` consumes public Rust `EnterpriseLinkIdentity` DTOs to resolve titles and redirect aliases across independently normalized chunks. Unlike the JSON-decoded Enterprise article join, it previously accepted typed generation labels such as `../wrong`, malformed Wikimedia project identifiers, revision numbers above SQLite's signed 64-bit range, unbounded revision timestamps, and invisible/bidirectional formatting controls embedded in article titles or redirects.

In `crates/wiki-source/src/enterprise_normalize_article.rs`, the constructor now rejects:
- Projects outside the shared conservative `valid_wiki_project` contract, before cross-project comparison.
- Generation IDs outside `enterprise_ndjson::valid_generation_id`, before index population.
- Zero or above-`i64::MAX` revision IDs.
- Blank, whitespace-framed, or more-than-`MAX_ENTERPRISE_RECORD_BYTES` revision timestamp strings.
- Canonical titles and redirect aliases failing the shared `valid_membership_label` grammar, including bidi overrides, zero-width spoofing characters, and overly long strings.

The existing exact project/generation/revision/date/title match in `validate_page` remains necessary; this patch validates the index's source declarations themselves rather than trusting a typed DTO constructor. Valid multilingual and joining-script text remains accepted.

## Regressions and CI evidence

New `link_index_revalidates_public_typed_source_identity_and_aliases` exercises invalid and valid cases at the public index boundary, including safe 64-bit revision bounds and spoofed canonical/alias labels. Existing cross-chunk target resolution, ambiguous title collision rejection, and revision/generation drift regressions continue to run.

- Initial code commit `628cb7af5ea9adfde5113fa0c5156dc02d115108`: failed `cargo fmt --all -- --check` at [Rust workspace run 40636105209](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40636105209).
- Corrected code SHA `83640747c40dc916ea58cb6af82afe04093d42f4`: [Rust workspace run 40636185975](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40636185975) **passed**, including formatting, strict Clippy and workspace tests.
- [Platform shells run 40636185925](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40636185925) on the same exact source SHA **passed**.

## Scope limits

This is a local typed-boundary validation improvement, not a proof that upstream Wikimedia published a complete Structured Contents/regular/deletion generation. It does not authenticate source pins, establish complete categories/redirects, resolve every media dependency, guarantee offline-ready visuals or qualify Android/desktop devices. SRC-002 remains unchecked until those independent requirements are fulfilled.
