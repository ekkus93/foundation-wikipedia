# SRC-002 — Canonical Wikidata Q-ID parity

**Date:** 2026-10-27 (America/Los_Angeles)  
**Repository:** `ekkus93/foundation-wikipedia`  
**Source code SHA:** `e48b50a3f5506d600a9582c4f06e87a84f820303`  
**Canonical model code SHA:** `527187f924a53d8c2a215ffba71354adbd670029`

## Problem and scope

The Enterprise Structured Contents decoder and public typed join accepted syntactically numeric but noncanonical Wikidata identities such as `Q0` and `Q01`, even though the canonical Article model already rejected them. The decoder and typed join also accepted arbitrarily long Q-IDs up to the overall record budget, and the model allowed such identifiers, while the digest-verified SQLite catalog lookup rejected any Q-ID longer than 64 bytes. As a result, an imported record could carry an ID that the reader's verified lookup could not query.

## Implemented changes

- `crates/wiki-source/src/enterprise.rs`: one shared private validator now gates both JSON `main_entity.identifier` parsing and public typed single/batch joins. A valid Q-ID is `Q` followed by at least one ASCII digit, not starting with zero, with total byte length at most 64. This is a **local identity rule**, not proof that the item exists at Wikidata.
- `crates/wiki-model/src/lib.rs`: `Article::validate` now enforces the same 64-byte upper bound in addition to its existing Q-prefix, digit and leading-zero checks.
- `crates/wiki-store/src/catalog.rs`: the existing verified Wikidata lookup already applied the 64-byte bound and canonical leading-zero rules; no behavioral change was required.

## Regression coverage and exact-head qualification

The Enterprise unit regressions now reject `Q0`, `Q01` and a 65-byte ID at JSON decoding; reject `Q0`, `Q01` and oversized caller-constructed typed IDs at the join boundary; and accept a 64-byte `Q` plus 63-digit boundary value. The canonical model regression rejects the 65-byte value and accepts the 64-byte value. The pre-existing catalog regression checks `Q0`, `Q01`, malformed IDs and valid verified lookups.

- Source code `e48b50a3f5506d600a9582c4f06e87a84f820303`: [Rust workspace 40633492468](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40633492468) **passed**; [Platform shells 40633492454](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40633492454) **passed**.
- Model code `527187f924a53d8c2a215ffba71354adbd670029`: [Rust workspace 40633725376](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40633725376) **passed**; [Platform shells 40633725419](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40633725419) **passed**; [Android Rust ABI 40633725346](https://github.com/ekkus93/foundation-wikipedia/actions/runs/40633725346) **passed**.

These checks validate code paths and compilation, not a real Android device or an authenticated publisher feed.

## Remaining acceptance

**SRC-002 remains unchecked.** Matching local Q-ID syntax is not evidence of actual Wikidata item existence, authenticated Wikimedia publisher identity, complete regular/structured/category/redirect/deletion joins, revision-matched HTML and required visual completeness, or an installed offline pack that works on desktop and Android. The full source publication and device acceptance gates remain outstanding.
