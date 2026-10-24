# PACK-005 — Curated SHA-256 digest spelling canonicalization

**Date:** 2026-10-24  
**Status:** Partial implementation qualification; PACK-005 remains unchecked.

## Defect and correction

`curated_definition_fingerprint` accepts an upstream 64-character SHA-256
rules-definition receipt in hexadecimal form. The same digest bytes can be
spelled with uppercase, lowercase or mixed-case A–F, but the previous
implementation hashed the *textual spelling*. Two callers with identical
topic IDs, seed definitions and effective selection rules could therefore
obtain different curated-definition fingerprints.

The public API now validates the 64-character hexadecimal receipt as before
and hashes its lowercase canonical spelling. The existing domain separator,
length-delimited fields, canonical topic ID order and editable seed-policy
coverage remain unchanged. Standard lowercase receipts are byte-for-byte
compatible with the earlier implementation.

## Regression and CI evidence

- Code SHA: `3851ef9f4a33a79c42a5f286b51c5c82d1dd9421`.
- `fingerprint_canonicalizes_rule_digest_hex_case` checks lowercase,
  uppercase and mixed-case encodings of the same digest, and a genuinely
  different digest.
- The existing invalid-digest regression also rejects a correctly sized
  non-hexadecimal value.
- Initial code SHA `c9c48a13bc2c21cf0d8921b2291ddf37013c8011`
  failed rustfmt at [Rust CI 40080279359](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40080279359);
  formatting was corrected in the code SHA above.
- [Rust workspace CI 40080322460](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40080322460)
  passed format, Clippy and workspace tests.
- [Platform shells CI 40080322413](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40080322413)
  was in progress at documentation time; confirm terminal result.

## Scope and remaining acceptance

The receipt still only fingerprints caller-supplied curated definitions and
effective local selection rules. It is not proof of an authenticated Wikimedia
publication, complete category coverage, signed pack construction, required
offline media, or physical desktop/Android acceptance. PACK-005 stays open.
